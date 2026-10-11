//! The Neo4j GQLSTATUS codes lpg raises where the ISO conditions insert
//! defines no condition (`interfaces.md` "GQLSTATUS codes"; rmp task #39,
//! point 5).
//!
//! Listed by hand. Each row's subcondition text is Neo4j's (R-NEO
//! `community/neo4j-gql-status/src/main/java/org/neo4j/gqlstatus/GqlStatusInfoCodes.java`
//! @ `54a7dcf`, line in the comment), and its class name is Neo4j's
//! classification of the class (`Condition.java:44-63` of the same
//! directory). The documentation of each constant links the Neo4j Status
//! Codes page of its code, checked on 2026-10-08 (22N27 and 42N25 on
//! 2026-10-11). The class categories 50 and 51 are not in the ISO
//! insert; Neo4j classifies both as exceptions.

use super::{Condition, GqlStatus, Severity};

macro_rules! neo4j_table {
    ($( $ix:literal $konst:ident $code:literal $class:literal $sub:literal $line:literal; )*) => {
        /// Every Neo4j code lpg raises, in code order.
        pub(crate) static CONDITIONS: &[Condition] = &[
            $( Condition {
                code: $code,
                severity: Severity::Exception,
                class: $class,
                subclass: Some($sub),
            }, )*
        ];

        /// One constant per Neo4j code: `C` followed by the code.
        pub mod codes {
            use super::GqlStatus;
            $(
                #[doc = concat!("`", $code, "` ", $class, ", ", $sub,
                    " (Neo4j `GqlStatusInfoCodes.java:", $line, "`; ",
                    "<https://neo4j.com/docs/status-codes/current/errors/gql-errors/",
                    $code, "/>).")]
                pub const $konst: GqlStatus = GqlStatus(super::super::generated::ISO_COUNT + $ix);
            )*
        }
    };
}

neo4j_table! {
    0 C08N09 "08N09" "connection exception" "database unavailable" 352;
    1 C22N11 "22N11" "data exception" "invalid argument" 601;
    2 C22N27 "22N27" "data exception" "invalid entity type" 697;
    3 C22N51 "22N51" "data exception" "graph reference not found" 839;
    4 C22N64 "22N64" "data exception" "constraint does not exist" 911;
    5 C22N69 "22N69" "data exception" "index does not exist" 938;
    6 C22N77 "22N77" "data exception" "property presence verification failed" 979;
    7 C22N78 "22N78" "data exception" "property type verification failed" 991;
    8 C22N79 "22N79" "data exception" "property uniqueness constraint violated" 1003;
    9 C25N03 "25N03" "invalid transaction state" "concurrent access violation" 1457;
    10 C25N05 "25N05" "invalid transaction state" "transaction closed" 1466;
    11 C25N11 "25N11" "invalid transaction state" "conflicting transaction state" 1483;
    12 C25N14 "25N14" "invalid transaction state" "transaction termination client error" 1497;
    13 C42N02 "42N02" "syntax error or access rule violation" "writing in read access mode" 2052;
    14 C42N08 "42N08" "syntax error or access rule violation" "no such procedure" 2090;
    15 C42N11 "42N11" "syntax error or access rule violation" "graph reference already exists" 2150;
    16 C42N25 "42N25" "syntax error or access rule violation" "missing YIELD" 2224;
    17 C42N48 "42N48" "syntax error or access rule violation" "no such function" 2341;
    18 C42NFF "42NFF" "syntax error or access rule violation" "permission/access denied" 2797;
    19 C50N00 "50N00" "general processing exception" "internal error" 2812;
    20 C50N05 "50N05" "general processing exception" "deadlock detected" 2833;
    21 C51N26 "51N26" "system configuration or operation exception" "not supported in this version" 3118;
    22 C51N40 "51N40" "system configuration or operation exception" "unable to start database" 3196;
    23 C51N41 "51N41" "system configuration or operation exception" "admin operation failed" 3201;
    24 C51N66 "51N66" "system configuration or operation exception" "resource exhaustion" 3347;
    25 C51N72 "51N72" "system configuration or operation exception" "memory pool out of memory" 3380;
    26 C51N73 "51N73" "system configuration or operation exception" "transaction memory limit reached" 3387;
}
