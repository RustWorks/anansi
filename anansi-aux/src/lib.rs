use std::fmt;
use std::any::Any;
use std::rc::Rc;
use std::slice::{Iter, IterMut};
use std::cell::{RefCell, Ref, RefMut};
use std::collections::HashMap;
use std::error::Error;
use std::marker::PhantomData;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, Node, NodeList, Document, Text, Window};

use serde_json::Value;
use serde::{Serialize, Serializer, ser::SerializeSeq, Deserialize, de::DeserializeOwned};
use serde::de::{Deserializer, Visitor, SeqAccess};

pub use anansi_macros::*;

extern crate self as anansi_aux;

pub mod prelude {
    pub use serde_json::Value;
    pub use serde::{Serialize, Deserialize};
    pub use anansi_macros::{store, Properties, component, function_component, refchild, release};
    pub use super::{attributes, element, Rsx, Sub, Proxy, Comp, Elem, Attribute, CbCmd, Resource, Rendered, RefVec, RefChild, Signal};
}

pub mod components;

pub type Mounts = &'static [(&'static str, fn(String), fn())];

thread_local! {
    pub static WINDOW: Window = web_sys::window().expect("should have a window");
    pub static DOCUMENT: Document = {
        let window = web_sys::window().expect("should have a window");
        window.document().expect("window should have a document")
    };
    pub static CALLBACKS: RefCell<HashMap<String, CallbackData>> = RefCell::new(HashMap::new());
    pub static RECALLS: RefCell<HashMap<String, RecallData>> = RefCell::new(HashMap::new());
    pub static APP_STATE: RefCell<Option<AppState>> = RefCell::new(None);
    pub static NODE_ID: RefCell<String> = RefCell::new(String::new());
    pub static IDS: RefCell<Vec<String>> = RefCell::new(vec![]);
    pub static RID: RefCell<usize> = RefCell::new(0);
    pub static CONTEXTS: RefCell<HashMap<String, Ctx>> = RefCell::new(HashMap::new());
    pub static REFS: RefCell<HashMap<usize, Vec<usize>>> = RefCell::new(HashMap::new());
    pub static COMP_RSX: RefCell<HashMap<CompId, Option<Rsx>>> = RefCell::new(HashMap::new());
    pub static VNODE_MAP: RefCell<HashMap<String, Node>> = RefCell::new(HashMap::new());
}

#[derive(PartialEq, Eq, Hash, Debug)]
pub struct CompId {
    node_id: String,
    n: usize,
}

impl CompId {
    pub fn new(node_id: String, n: usize) -> Self {
        Self {node_id, n}
    }
    pub fn node_id(&self) -> &String {
        &self.node_id
    }
    pub fn n(&self) -> usize {
        self.n
    }
}

#[macro_export]
macro_rules! comp_statics {
    ($($name:expr,)*) => {
        pub static STATICS: &[(&'static str, &'static [u8])] = &[
            $((concat!("/static/styles/", $name, ".css"), include_bytes!(concat!("static", anansi_aux::main_separator!(), "styles", anansi_aux::main_separator!(), $name, ".css"))),)*
        ];
    }
}

#[macro_export]
#[cfg(not(target_os = "windows"))]
macro_rules! main_separator {
    () => {r"/"}
}

#[macro_export]
#[cfg(target_os = "windows")]
macro_rules! main_separator {
    () => {r"\"}
}

pub fn load_style(url: &'static str) {
    DOCUMENT.with(|document| {
        if let Ok(links) = document.query_selector_all("link") {
            for i in 0..links.length() {
                if let Some(node) = links.get(i) {
                    let link = node.dyn_ref::<Element>().unwrap();
                    if let Some(href) = link.attributes().get_named_item("href") {
                        if href.value() == url {
                            return;
                        }
                    }
                }
            }
        }
        if let Ok(head) = document.query_selector("head") {
            if let Some(head) = head {
                if let Ok(link) = document.create_element("link") {
                    link.set_attribute("rel", "stylesheet").unwrap();
                    link.set_attribute("href", url).unwrap();
                    head.append_child(&link).unwrap();
                }
            }
        }
    });
}

#[derive(Debug)]
pub enum CbCmd {
    Callback(u8),
    Text(u8, Result<String, Box<dyn Error>>),
}

#[derive(Properties, Serialize, Deserialize)]
pub struct EmptyProp;

#[derive(Debug)]
pub enum Resource<D> {
    Pending,
    Rejected(Box<dyn Error>),
    Resolved(D),
}

pub struct Rendered(Vec<Rsx>);

impl Rendered {
    pub fn new(rsx: Vec<Rsx>) -> Self {
        Self(rsx)
    }
    pub fn resume(_store: &mut AppState, _n: usize) -> Self {
        Self(vec![])
    }
    pub fn rsx(&self) -> &Vec<Rsx> {
        &self.0
    }
}

pub enum Cmd {
    Update(Rsx, String),
    Set(HashMap<String, Ctx>),
}

pub trait RefChild {
    type Item;
    fn new(pos: usize, item: Self::Item) -> Self;
    fn pos(&self) -> usize;
    fn pos_mut(&mut self) -> &mut usize;
}

impl<T: Serialize> Serialize for RefVec<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for e in &self.0 {
            seq.serialize_element(&*e.borrow())?;
        }
        seq.end()
    }
}

struct RefVecVisitor<T> {
    t: PhantomData<T>,
}

impl<'de, T: Deserialize<'de> + RefChild> Visitor<'de> for RefVecVisitor<T> {
    type Value = RefVec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("struct RefVec")
    }

    fn visit_seq<V>(self, mut seq: V) -> Result<RefVec<T>, V::Error>
    where
        V: SeqAccess<'de>,
    {
        let mut new_obj = RefVec::new();
        while let Some(value) = seq.next_element()? {
            new_obj.push_ref(value);
        }

        Ok(new_obj)
    }
}

impl<'de, T: Deserialize<'de> + RefChild> Deserialize<'de> for RefVec<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(RefVecVisitor {t: PhantomData})
    }
}

pub trait Parent {
    type Item;
}

#[derive(Debug)]
pub struct RefVec<T: ?Sized>(Vec<Rc<RefCell<T>>>);

impl<T> Parent for RefVec<T> {
    type Item = T;
}

impl<T: ?Sized> RefVec<T> {
    pub fn new() -> Self {
        Self(vec![])
    }
    pub fn inner(&self) -> &Vec<Rc<RefCell<T>>> {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

pub trait GetOne {
    fn get_one(&self, n: usize) -> Rc<dyn Any>;
}

impl<T: 'static> GetOne for RefVec<T> {
    fn get_one(&self, n: usize) -> Rc<dyn Any> {
        self.0[n].clone() as Rc<dyn Any>
    }
}

impl<T: RefChild> RefVec<T> {
    pub fn push(&mut self, t: <T as RefChild>::Item) {
        self.0.push(Rc::new(RefCell::new(T::new(self.0.len(), t))));
    }
    pub fn push_ref(&mut self, t: T) {
        self.0.push(Rc::new(RefCell::new(t)));
    }
    pub fn append(&mut self, t: &mut Vec<<T as RefChild>::Item>) {
        let v = t.split_off(0);
        let mut n = self.0.len();
        for e in v {
            self.0.push(Rc::new(RefCell::new(T::new(n, e))));
            n += 1;
        }
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn swap(&mut self, a: usize, b: usize) {
        self.0.swap(a, b);
    }
    pub fn remove(&mut self, index: usize) -> Rc<RefCell<T>> {
        let mut rest = self.0.split_off(index + 1);
        let removed = self.0.pop().unwrap();
        for c in &mut rest {
            *c.borrow_mut().pos_mut() -= 1;
        }
        self.0.append(&mut rest);
        removed
    }
    pub fn iter(&self) -> RefIter<'_, T> {
        RefIter {iter: self.0.iter()}
    }
    pub fn iter_mut(&mut self) -> RefIterMut<'_, T> {
        RefIterMut {iter_mut: self.0.iter_mut()}
    }
    pub fn with_capacity(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity))
    }
}

pub struct RefIter<'a, T> {
    iter: Iter<'a, Rc<RefCell<T>>>,
}

impl<'a, T> Iterator for RefIter<'a, T> {
    type Item = Ref<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(r) = self.iter.next() {
            Some(r.borrow())
        } else {
            None
        }
    }
}

pub struct RefIterMut<'a, T> {
    iter_mut: IterMut<'a, Rc<RefCell<T>>>,
}

impl<'a, T> Iterator for RefIterMut<'a, T> {
    type Item = RefMut<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(r) = self.iter_mut.next() {
            Some(r.borrow_mut())
        } else {
            None
        }
    }
}

pub struct Signal<T> {
    _proxy: SignalProxy,
    value: T,
}

impl<T> Parent for Signal<T> {
    type Item = T;
}

impl<T: Serialize + DeserializeOwned + 'static + std::fmt::Debug> Signal<T> {
    pub fn resume(store: &mut AppState, n: usize) -> Self {
        if let Obj::Js(v) = &store.objs[n] {
            let t: T = serde_json::from_value(v.clone()).unwrap();
            let subs = store.subs.pop().expect("problem getting subs");
            Self {_proxy: SignalProxy::from(subs[0]), value: t}
        } else {
            panic!("expected JavaScript value when resuming")
        }
    }
}

impl<T> Signal<T> {
    pub fn new(t: T) -> Self {
        Self {_proxy: SignalProxy::new(), value: t}
    }
    pub fn value(&mut self) -> &T {
        self._proxy.set();
        &self.value
    }
    pub fn value_mut(&mut self) -> &mut T {
        self._proxy._invalid = true;
        &mut self.value
    }
    pub fn get_subs(&self) -> Vec<String> {
        self._proxy.get_subs()
    }
    pub fn into_inner(self) -> T {
        self.value
    }
}

#[derive(Clone)]
pub struct SignalProxy {
    pub _learning: bool,
    pub _invalid: bool,
    pub _node: u32,
    pub _dirty: i64,
    pub _sub: Sub,
}

impl SignalProxy {
    pub fn new() -> Self {
        Self {_learning: false, _invalid: false, _node: 0, _dirty: -1, _sub: (0, 0)}
    }
    pub fn from(_sub: (u32, i64)) -> Self {
        Self {_learning: false, _invalid: false, _node: 0, _dirty: -1, _sub}
    }
    pub fn set(&mut self) {
        if self._learning {
            self._sub = (self._node, 0);
        } else {
            if self._dirty == -1 {
                self._dirty = 0;
            }
            self._dirty |= 1;
        }
    }
    pub fn start_proxy(&mut self) -> Sub {
        self._learning = true;
        self._invalid = false;
        self._dirty = -1;
        self._sub
    }
    pub fn stop_proxy(&mut self, sub: Sub) {
        self._sub = sub;
        self._learning = false;
    }
    pub fn get_subs(&self) -> Vec<String> {
        vec![format!("{} {}", self._sub.0, self._sub.1)]
    }
}

pub struct Proxy {
    pub _learning: bool,
    pub _invalid: bool,
    pub _node: u32,
    pub _dirty: i64,
    pub _subs: Vec<Sub>,
}

impl Proxy {
    pub fn new(subs: Vec<Sub>) -> Self {
        Self {_learning: false, _invalid: false, _node: 0, _dirty: -1, _subs: subs}
    }
    pub fn set(&mut self, n: i64) {
        if self._learning {
            self._subs.push((self._node, n));
        } else {
            if self._dirty == -1 {
                self._dirty = 0;
            }
            self._dirty |= n;
        }
    }
    pub fn start_proxy(&mut self) -> Vec<Sub> {
        self._learning = true;
        self._invalid = false;
        self._dirty = -1;
        let mut subs = vec![];
        subs.append(&mut self._subs);
        subs
    }
    pub fn stop_proxy(&mut self, subs: Vec<Sub>) {
        self._subs = subs;
        self._learning = false;
    }
    pub fn get_subs(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(self._subs.len());
        for sub in &self._subs {
            v.push(format!("{} {}", sub.0, sub.1));
        }
        v
    }
}

#[derive(Debug, Clone)]
pub struct Comp {
    pub children: Vec<Rsx>,
}

#[derive(Debug, Clone)]
pub struct Elem {
    pub name: &'static str,
    pub attrs: Vec<Attribute>,
    pub children: Vec<Rsx>,
}

#[derive(Debug, Clone)]
pub struct Attribute {
    pub key: String,
    pub value: String,
}

#[macro_export]
macro_rules! attributes {
    ($(($k:expr, $v:expr)),* $(,)?) => {
        vec![$(Attribute {key: $k, value: $v},)*]
    }
}

impl Elem {
    fn to_node(&self, document: &Document) -> Node {
        let el = document.create_element(self.name).unwrap();
        for attr in &self.attrs {
            if !attr.key.starts_with("on:") {
                el.set_attribute(&attr.key, &attr.value).unwrap();
            } else {
                CALLBACKS.with(|c| {
                    let c = c.borrow();
                    let (v, _) = attr.value.split_once('[').unwrap();
                    let cb = c.get(v).unwrap();
                    RID.with(|r| {
                        let mut r = r.borrow_mut();
                        let rs = r.to_string();
                        el.set_attribute("rid", &rs).unwrap();
                        RECALLS.with(|rc| {
                            rc.borrow_mut().insert(rs, RecallData {call: cb.call});
                        });
                        *r += 1;
                    });
                });
            }
        }
        for child in &self.children {
            el.append_child(&child.to_node(document)).unwrap();
        }
        el.dyn_into::<Node>().unwrap()
    }
    fn diff(&self, node: &mut Node) {
        if self.name == node.node_name() {
            let el = node.dyn_ref::<Element>().unwrap();
            let attributes = el.attributes();
            if self.attrs.len() as u32 == attributes.length() {
                let mut same = true;
                for attr in &self.attrs {
                    if let Some(attribute) = attributes.get_named_item(&attr.key) {
                        if attribute.value() != attr.value {
                            same = false;
                            break;
                        }
                    }
                }
                if same {
                    return;
                }
            }
        } else {
            let parent = node.parent_node().unwrap();
            DOCUMENT.with(|document| {
                let new = self.to_node(&document);
                parent.insert_before(&new, Some(&node)).unwrap();
                *node = new;
            });
        }
    }
}

#[macro_export]
macro_rules! element {
    ($n:literal, $a:expr, $c: expr) => {
        Rsx::Element(Elem {name: $n, attrs: $a, children: $c})
    }
}

#[derive(Debug, Clone)]
pub enum Rsx {
    Component(Comp),
    Element(Elem),
    Text(String),
}

impl Rsx {
    fn edit(&self, node: &Node) {
        DOCUMENT.with(|document| {
            match self {
                Self::Element(elem) => {
                    let new = elem.to_node(&document);
                    add_sibling(node, &new);
                }
                Self::Text(text) => {
                    let new = document.create_text_node(&text).dyn_into::<Node>().unwrap();
                    add_sibling(node, &new);
                }
                Self::Component(_) => unimplemented!(),
            }
        });
    }
    fn to_node(&self, document: &Document) -> Node {
        match self {
            Self::Element(elem) => {
                elem.to_node(document)
            }
            Self::Text(text) => {
                document.create_text_node(&text).dyn_into::<Node>().unwrap()
            }
            Self::Component(_) => unimplemented!(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Ctx {
    R(String),
}

pub type Sub = (u32, i64);

pub struct AppState {
    objs: Vec<Obj>,
    subs: Vec<Vec<Sub>>,
}

impl AppState {
    pub fn objs(&self) -> &Vec<Obj> {
        &self.objs
    }
    pub fn objs_mut(&mut self) -> &mut Vec<Obj> {
        &mut self.objs
    }
    pub fn subs(&self) -> &Vec<Vec<Sub>> {
        &self.subs
    }
    pub fn subs_mut(&mut self) -> &mut Vec<Vec<Sub>> {
        &mut self.subs
    }
}

#[derive(Clone)]
pub enum Obj {
    Rs(Rc<RefCell<dyn Any>>),
    Js(Value),
}

pub fn html_escape(s: &str) -> String {
    let mut escaped = String::new();
    for c in s.chars() {
        let html = match c {
            '<' => "&lt;",
            '>' => "&gt;",
            '\'' => "&#x27;",
            '"' => "&quot;",
            '&' => "&amp;",
            _ => {
                escaped.push(c);
                continue;
            }
        };
        escaped.push_str(html);
    }
    escaped
}

pub struct RecallData {
    pub call: fn(),
}

pub struct CallbackData {
    pub new: fn(String),
    pub call: fn(),
    pub is_mounted: bool,
}

#[macro_export]
macro_rules! log {
    ($f:literal $($t:tt)*) => {
        web_sys::console::log_1(&format!($f $($t)*).into());
    };
    ($($t:tt)*) => {
        web_sys::console::log_1(&$($t)*);
    };
}

fn add_sibling(node: &Node, new: &Node) {
    match node.node_type() {
        Node::ELEMENT_NODE => node.dyn_ref::<Element>().unwrap().after_with_node_1(new).unwrap(),
        Node::TEXT_NODE => node.dyn_ref::<Text>().unwrap().after_with_node_1(new).unwrap(),
        _ => unimplemented!(),
    }
}

pub fn get_state(document: &Document, ctx_map: &mut HashMap<String, Ctx>) -> Option<AppState> {
    let script = document.query_selector_all("script[type='app/json']").unwrap().get(0).unwrap();
    let text = script.text_content().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let values = json.as_object().unwrap();
    let ctx = values.get("ctx").unwrap();
    let contexts = ctx.as_object().unwrap();
    let mut cmap = HashMap::new();
    for (id, n) in contexts {
        let c = serde_json::from_value(n.clone()).unwrap();
        cmap.insert(id.to_string(), c);
    }
    let object_array = values.get("objs").unwrap();
    let mut objs = vec![];
    for object in object_array.as_array().unwrap() {
        objs.push(Obj::Js(object.clone()));
    }
    let sub_array = values.get("subs").unwrap();
    let mut subs = vec![];
    for arr in sub_array.as_array().unwrap() {
        let mut sv = vec![];
        for sub in arr.as_array().unwrap() {
            let s = sub.as_str().unwrap();
            let nums: Vec<&str> = s.split(' ').collect();
            sv.push((nums[0].parse().unwrap(), nums[1].parse().unwrap()));
        }
        subs.push(sv);
    }
    let parent = script.parent_node().unwrap();
    parent.remove_child(&script).unwrap();
    *ctx_map = cmap;
    Some(AppState {objs, subs})
}

fn check_vnodes(nodes: &NodeList, vnode_map: &mut HashMap<String, Node>) {
    let mut ident;
    for i in 0..nodes.length() {
        let node = nodes.get(i).unwrap();
        if node.node_type() == Node::COMMENT_NODE {
            let comment = node.text_content().unwrap();
            if comment.starts_with("av ") {
                let attrs: Vec<&str> = comment.split(' ').collect();
                let mut id = false;
                for attr in &attrs[1..] {
                    let (k, v) = attr.split_once('=').unwrap();
                    if k == "a:id" {
                        ident = v.to_string();
                        vnode_map.insert(ident, node.clone());
                        id = true;
                        break;
                    }
                }
                if !id {
                    panic!("expected id for virtual node");
                }
            }
        } else {
            check_vnodes(&node.child_nodes(), vnode_map);
        }
    }
}
   
pub fn setup(callbacks: HashMap<String, CallbackData>) {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    CALLBACKS.with(|c| {
        let mut cb = c.borrow_mut();
        *cb = callbacks;
    });
}

pub fn rerender(rsx: Rsx) {
    CONTEXTS.with(|contexts| {
        let contexts = contexts.borrow();
        VNODE_MAP.with(|vnode_map| {
            let mut vnode_map = vnode_map.borrow_mut();
            NODE_ID.with(|node_id| {
                DOCUMENT.with(|document| {
                    let nodes = document.body().unwrap().child_nodes();
                    check_vnodes(&nodes, &mut vnode_map);
                    let vn_index = match contexts.get(&*node_id.borrow()).unwrap() {
                        Ctx::R(s) => s,
                    };
                    let mut node = vnode_map.get(vn_index).unwrap().clone().next_sibling().unwrap();
                    update(&rsx, &mut node);
                    close_vnode(&document, &node);
                });
            });
        });
    });
}

#[wasm_bindgen]
pub fn recall(rid: &str) -> bool {
    let mut b = false;
    RECALLS.with(|r| {
        let recalls = r.borrow();
        if let Some(rc) = recalls.get(rid) {
            let r = rc.call;
            drop(recalls);
            (r)();
            b = true;
        }
    });
    b
}

#[wasm_bindgen]
pub fn call(callback: &str, node_id: &str) -> Result<(), JsValue> {
    let (name, arr) = callback.split_once('[').unwrap();
    let (arr, _) = arr.rsplit_once(']').unwrap();
    let arr: Vec<String> = arr.split(' ').map(|s| s.to_string()).collect();

    CALLBACKS.with(|c| {
        let mut callbacks = c.borrow_mut();
        if let Some(cb) = callbacks.get_mut(name) {
            NODE_ID.with(|n| *n.borrow_mut() = node_id.to_string());
            IDS.with(|id| {
                *id.borrow_mut() = arr;
            });
            if !cb.is_mounted {
                (cb.new)(node_id.to_string());
                cb.is_mounted = true;
            }
            (cb.call)();
        }
    });

    Ok(())
}

pub fn lexical_scope() -> Vec<Rc<RefCell<dyn Any>>> {
    let mut v = vec![];
    APP_STATE.with(|app| {
        let app = app.borrow();
        IDS.with(|ids| {
            for id in ids.borrow().iter() {
                if let Some((f, s)) = id.split_once('-') {
                    let f: usize = f.parse().expect("problem parsing id for lexical scope");
                    let s: usize = s.parse().expect("problem parsing index for lexical scope");
                    if let Obj::Rs(var) = &app.as_ref().expect("could not get app state").objs[f] {
                        let mut var = var.borrow_mut();
                        let var = var.downcast_mut::<Signal<RefVec<dyn Any>>>().expect("problem getting reference for lexical scope");
                        let var = var.value().inner()[s].clone();
                        v.push(var);
                    } else {
                        panic!("expected Rust type to be restored");
                    }
                } else {
                    let id: usize = id.parse().expect("problem parsing id for lexical scope");
                    if let Obj::Rs(var) = &app.as_ref().expect("could not get app state").objs[id] {
                        v.push(var.clone());
                    } else {
                        panic!("expected Rust type to be restored");
                    }
                }
            }
        })
    });
    v
}

fn update(rsx: &Rsx, node: &mut Node) {
    match rsx {
        Rsx::Element(element) => {
            element.diff(node);
            if let Some(mut first_child) = node.first_child() {
                check_siblings(&element.children, &mut first_child);
            }
        }
        Rsx::Text(text) => {
            set_content(node, &text);
        }
        Rsx::Component(comp) => {
            check_siblings(&comp.children, node);
        }
    }
}

fn check_siblings(children: &Vec<Rsx>, node: &mut Node) {
    let mut children = children.iter();
    let l = children.len();
    let mut n = 0;

    loop {
        if let Some(child) = children.next() {
            update(child, node);
            
            if let Some(sib) = node.next_sibling() {
                if sib.node_type() == Node::COMMENT_NODE && sib.text_content().unwrap() == "/av" {
                    while let Some(c) = children.next() {
                        c.edit(&node);
                        *node = node.next_sibling().unwrap();
                    }
                    return;
                }

                if n < l - 1 {
                    *node = sib;
                }
            } else {
                if n < l - 1 {
                    child.edit(&node);
                    while let Some(c) = children.next() {
                        if let Some(sib) = node.next_sibling() {
                            *node = sib;
                            c.edit(&node);
                        } else {
                            c.edit(&node);
                            while let Some(d) = children.next() {
                                d.edit(&node);
                            }
                            return;
                        }
                    }
                }
                return;
            };
        } else {
            if let Some(s) = node.next_sibling() {
                let parent = node.parent_node().unwrap();
                RECALLS.with(|r| {
                    let mut recall = r.borrow_mut();
                    remove_recall(&mut recall, &parent, &s);
                    while let Some(sib) = node.next_sibling() {
                        remove_recall(&mut recall, &parent, &sib);
                    }
                });
            }
            return;
        }
        n += 1;
    }
}

fn remove_recall(recalls: &mut HashMap<String, RecallData>, parent: &Node, child: &Node) {
    if child.node_type() == Node::ELEMENT_NODE {
        let el = child.dyn_ref::<Element>().unwrap();
        let attrs = el.attributes();
        if let Some(rid) = attrs.get_named_item("rid") {
            recalls.remove(&rid.value());
        }
    }
    parent.remove_child(child).unwrap();
}

fn replace_recall(recalls: &mut HashMap<String, RecallData>, parent: &Node, child: &Node, new: &Node) {
    if child.node_type() == Node::ELEMENT_NODE {
        let el = child.dyn_ref::<Element>().unwrap();
        let attrs = el.attributes();
        if let Some(rid) = attrs.get_named_item("rid") {
            recalls.remove(&rid.value());
        }
    }
    parent.replace_child(new, child).unwrap();
}

fn set_content(node: &mut Node, content: &str) {
    let text = Text::new_with_data(content).unwrap();
    let parent = node.parent_node().unwrap();
    RECALLS.with(|r| {
        let mut recall = r.borrow_mut();
        let text_node = text.dyn_into::<Node>().unwrap();
        replace_recall(&mut recall, &parent, node, &text_node);
        *node = text_node;
    });
}

fn close_vnode(document: &Document, node: &Node) {
    if let Some(n) = node.next_sibling() {
        if n.node_type() == Node::COMMENT_NODE && n.text_content().unwrap() != "/av" {
            let c = document.create_comment("/av").dyn_into::<Node>().unwrap();
            add_sibling(&n, &c);
        }
    }
}
