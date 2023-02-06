---
# Feel free to add content and custom Front Matter to this file.
# To modify the layout, see https://jekyllrb.com/docs/themes/#overriding-theme-defaults

layout: page
title: Overview
---

Anansi is a simple full-stack web framework for Rust. [Get started](/anansi/start).

🛡️ Safety first
---------------

In addition to being written in Rust, Anansi provides defenses for common web security vulnerabilities.

⚙️  Performant
-------------

Anansi also allows web applications to run asynchronously with Rust's speed.

✨ Easy to get started
----------------------

Anansi handles many of the repetitive parts of web development, letting you work on the important parts of your app more quickly.

Records
=======

Work with databases in Rust instead of SQL to write statically checked queries.

```rust
// A topic in a forum.
#[record]
#[derive(Relate, FromParams, ToUrl)]
pub struct Topic {
    pub title: VarChar<200>,
    pub user: ForeignKey<User>,
    pub content: VarChar<40000>,
    pub date: DateTime,
}

// A comment in a topic.
#[record]
#[derive(Relate, FromParams)]
pub struct Comment {
    pub topic: ForeignKey<Topic>,
    pub user: ForeignKey<User>,
    pub content: VarChar<40000>,
    pub date: DateTime,
}
```

Views
=====

Mapping requests to views is simple.

```rust
routes! {
    path!("", TopicView::index),
    path!("new", TopicView::new),
    path!("load", TopicView::load),
    path!("{topic_id}", TopicView::show),
}
```

```rust
#[viewer]
impl<R: Request> TopicView<R> {
    // A view of the last 25 topics.
    #[view(Site::is_visitor)]
    pub async fn index(req: &mut R) -> Result<Response> {
        let title = "Latest Topics";
        let topics = Topic::order_by(date().desc())
    	    .limit(25).query(req).await?;
        let show_url = url!(req, Self::show);
        let load_url = url!(req, Self::load);
    }
}
```

Templates
=========

Templates allow you to mix Rust with HTML for formatting.

```rust
@block title {@title}

@block content {
    @load components {
        <h1>@title</h1>
        @if req.user().is_auth() {
            @link req, Self::new {New Topic}
        }
        <ul>
            @for topic in &topics {
    	        <li>@link req, Self::show, topic {@topic.title}</li>
            }
            @if topics.len() == 25 {
                <Loader @show_url @load_url />
            }
        </ul>
    }
}
```

Components
==========

Reactivity can be added with WebAssembly.

```rust
#[derive(Properties, Serialize, Deserialize)]
pub struct LoaderProps {
    pub load_url: String,
    pub show_url: String,
}

#[derive(Serialize, Deserialize)]
pub struct Data {
    pub id: String,
    pub title: String,
}

#[store]
#[derive(Serialize, Deserialize)]
pub struct Loader {
    visible: bool,
    page: u32,
    fetched: Vec<Data>,
}

#[component(Loader)]
fn init(props: LoaderProps) -> Rsx {
    let state = Self::store(true, 1, vec![]);

    let (data_resource, handle_click) = resource!(Vec<Data>, state, props, {
        *state.visible_mut() = false;
        Request::get(&props.load_url)
            .query([("page", state.page().to_string())])
    });

    rsx!(state, props, data_resource, {
        @for data in state.fetched() {
            <li>@href props.show_url, data.id {@data.title}</li>
        }
        @resource data_resource, state {
            Resource::Pending => {
                <Spinner />
            }
            Resource::Rejected(_) => {
                *state.visible_mut() = true;
                <div>Problem loading topics</div>
            }
            Resource::Resolved(mut f) => {
                if f.len() == 25 && *state.page() < 3 {
                    *state.page_mut() += 1;
                    *state.visible_mut() = true;
                }
                state.fetched_mut().append(&mut f);
            }
        }
        @if *state.visible() {
            <button @onclick(handle_click)>Load more</button>
        }       
    }
}
```

Scoped CSS
==========

CSS can be isolated to individual components.

```rust
#[function_component(Spinner)]
fn init() -> Rsx {
    style! {
        div {
            display: inline-block;
            width: 25px;
            height: 25px;
            border: 3px solid #cfd0d1;
            border-radius: 50%;
            border-top-color: #1c87c9;
            animation: spin 1s ease-in-out infinite;
            -webkit-animation: spin 1s ease-in-out infinite;
        }
        @keyframes spin {
            to {
                -webkit-transform: rotate(360deg);
            }
        }
        @-webkit-keyframes spin {
            to {
                -webkit-transform: rotate(360deg);
            }
        }
    }

    rsx! {
        <div></div>
    }
}
```
