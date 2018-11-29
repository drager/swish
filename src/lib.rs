//! # Swish
//!
//! Rust API bindings for the [Swish API](https://developer.getswish.se/merchants/).
//! Built using [hyper](https://docs.rs/hyper/0.12.16/hyper/) and [tokio](https://docs.rs/tokio-core/0.1.17/tokio_core/).
//!
extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate native_tls;
extern crate serde;
extern crate tokio_core;

#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate serde_derive;

pub mod client;
pub mod error;
