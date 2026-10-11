//! Tests of the public API (rmp #47 to #51). Each async test runs on a
//! `current_thread` and on a `multi_thread` runtime ([`both`]), with its
//! graph data under `/tmp/lpgram` (CLAUDE.md). Concurrent steps are
//! ordered by pause points (channels a procedure signals on), never by
//! sleeping.

mod api;
mod auth;
mod bulk;
mod limits;
mod obs;
mod procs;

use std::future::Future;
use std::path::Path;

use crate::auth::{Action, Authoriser, BoxFuture, Object};
use crate::error::Error;
use crate::obs::recording::RecordingSink;
use crate::sync::atomic::{AtomicUsize, Ordering};
use crate::sync::{Arc, Mutex};
use crate::test_support::{OWNER, no_flush};
use crate::types::Value;
use crate::{Database, Options, Parameters, Session};

/// The memory cap of these tests: 32 MiB.
pub(super) const CAP: u64 = 32 << 20;

/// Runs the future `f` makes on a `current_thread` runtime, then on a
/// `multi_thread` one (rmp #47 acceptance).
pub(super) fn both<F, Fut>(f: F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = ()>,
{
    current(f());
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .expect("a multi_thread runtime")
        .block_on(f());
}

/// Runs `f` on a `current_thread` runtime.
pub(super) fn current<Fut: Future<Output = ()>>(f: Fut) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a current_thread runtime")
        .block_on(f);
}

/// A refusal rule of [`TestAuth`].
type Rule = (Option<String>, Action, Option<Object>);

/// An authoriser that records every request and refuses those its rules
/// match.
#[derive(Default)]
pub(super) struct TestAuth {
    /// Refused: principal (any if `None`), action, object (any if `None`).
    refused: Mutex<Vec<Rule>>,
    /// Every request: principal, action, object, owner.
    pub(super) log: Mutex<Vec<(String, Action, Object, String)>>,
    pub(super) authentications: AtomicUsize,
}

impl TestAuth {
    pub(super) fn new() -> Arc<TestAuth> {
        Arc::new(TestAuth::default())
    }

    /// Refuses `action` on `object` (any object if `None`) to `principal`
    /// (anyone if `None`).
    pub(super) fn refuse(&self, principal: Option<&str>, action: Action, object: Option<Object>) {
        self.refused
            .lock()
            .unwrap()
            .push((principal.map(str::to_string), action, object));
    }

    /// The requests so far.
    pub(super) fn requests(&self) -> Vec<(String, Action, Object, String)> {
        self.log.lock().unwrap().clone()
    }

    pub(super) fn clear(&self) {
        self.log.lock().unwrap().clear();
    }
}

impl Authoriser for TestAuth {
    fn authenticate<'a>(
        &'a self,
        _scheme: &'a str,
        _credentials: &'a str,
    ) -> BoxFuture<'a, Option<crate::auth::Authenticated>> {
        self.authentications.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { None })
    }

    fn authorise<'a>(
        &'a self,
        principal: &'a str,
        action: Action,
        object: &'a Object,
        owner: &'a str,
    ) -> BoxFuture<'a, bool> {
        let refused = self.refused.lock().unwrap().iter().any(|(p, a, o)| {
            p.as_deref().is_none_or(|p| p == principal)
                && *a == action
                && o.as_ref().is_none_or(|o| o == object)
        });
        self.log.lock().unwrap().push((
            principal.to_string(),
            action,
            object.clone(),
            owner.to_string(),
        ));
        Box::pin(async move { !refused })
    }
}

/// Options with `auth` and `sink`.
pub(super) fn options(auth: &Arc<TestAuth>, sink: &Arc<RecordingSink>) -> Options {
    Options::new(auth.clone()).with_metrics(sink.clone())
}

/// A new database in `dir`, owned by [`OWNER`].
pub(super) async fn create(dir: &Path, auth: &Arc<TestAuth>) -> Database {
    create_with(dir, auth, &Arc::new(RecordingSink::default()), CAP).await
}

/// A new database in `dir` with `sink` and `cap`.
pub(super) async fn create_with(
    dir: &Path,
    auth: &Arc<TestAuth>,
    sink: &Arc<RecordingSink>,
    cap: u64,
) -> Database {
    Database::create(dir, no_flush(), cap, OWNER, options(auth, sink))
        .await
        .expect("create")
}

/// A session of `principal` at the defaults.
pub(super) async fn session(db: &Database, principal: &str) -> Session {
    db.session(principal, None, None).await.expect("session")
}

/// Every row of `gql` in `s`.
pub(super) async fn rows(s: &Session, gql: &str) -> Vec<Vec<Value>> {
    rows_with(s, gql, Parameters::new())
        .await
        .unwrap_or_else(|e| panic!("{gql}: {e}"))
}

/// Every row of `gql` with `params` in `s`, or its error.
pub(super) async fn rows_with(
    s: &Session,
    gql: &str,
    params: Parameters,
) -> Result<Vec<Vec<Value>>, Error> {
    let mut st = s.execute(gql, params).await?;
    let mut out = Vec::new();
    while let Some(r) = st.next().await? {
        out.push(r);
    }
    Ok(out)
}

/// The error of `gql` in `s`.
pub(super) async fn error(s: &Session, gql: &str) -> Error {
    match rows_with(s, gql, Parameters::new()).await {
        Ok(r) => panic!("{gql}: expected an error, got {r:?}"),
        Err(e) => e,
    }
}

/// The single integer of a one-row, one-column result.
pub(super) async fn int(s: &Session, gql: &str) -> i64 {
    match rows(s, gql).await.as_slice() {
        [r] => match r.as_slice() {
            [Value::Int(i)] => *i,
            other => panic!("{gql}: {other:?}"),
        },
        other => panic!("{gql}: {other:?}"),
    }
}

/// One parameter.
pub(super) fn param(name: &str, v: Value) -> Parameters {
    let mut p = Parameters::new();
    p.insert(name.to_string(), v);
    p
}

/// A string value.
pub(super) fn string(x: &str) -> Value {
    Value::string(x)
}
