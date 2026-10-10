//! Generated from the keyword rules of `GQL.g4` (opengql/grammar @
//! `16ea71b`, lines 3276-3584). Do not edit by hand: run
//! `LPG_UPDATE_KEYWORDS=1 cargo test -p lpg
//! query::lexer::tests::keyword_table_matches_the_grammar`.

/// A keyword token of `GQL.g4`, named after its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Keyword {
    /// `ABS` (`ABS`)
    Abs,
    /// `ACOS` (`ACOS`)
    Acos,
    /// `ALL` (`ALL`)
    All,
    /// `ALL_DIFFERENT` (`ALL_DIFFERENT`)
    AllDifferent,
    /// `AND` (`AND`)
    And,
    /// `ANY` (`ANY`)
    Any,
    /// `ARRAY` (`ARRAY`)
    Array,
    /// `AS` (`AS`)
    As,
    /// `ASC` (`ASC`)
    Asc,
    /// `ASCENDING` (`ASCENDING`)
    Ascending,
    /// `ASIN` (`ASIN`)
    Asin,
    /// `AT` (`AT`)
    At,
    /// `ATAN` (`ATAN`)
    Atan,
    /// `AVG` (`AVG`)
    Avg,
    /// `BIG` (`BIG`)
    Big,
    /// `BIGINT` (`BIGINT`)
    Bigint,
    /// `BINARY` (`BINARY`)
    Binary,
    /// `BOOL` (`BOOL`)
    Bool,
    /// `BOOLEAN` (`BOOLEAN`)
    Boolean,
    /// `BOTH` (`BOTH`)
    Both,
    /// `BTRIM` (`BTRIM`)
    Btrim,
    /// `BY` (`BY`)
    By,
    /// `BYTE_LENGTH` (`BYTE_LENGTH`)
    ByteLength,
    /// `BYTES` (`BYTES`)
    Bytes,
    /// `CALL` (`CALL`)
    Call,
    /// `CARDINALITY` (`CARDINALITY`)
    Cardinality,
    /// `CASE` (`CASE`)
    Case,
    /// `CAST` (`CAST`)
    Cast,
    /// `CEIL` (`CEIL`)
    Ceil,
    /// `CEILING` (`CEILING`)
    Ceiling,
    /// `CHAR` (`CHAR`)
    Char,
    /// `CHAR_LENGTH` (`CHAR_LENGTH`)
    CharLength,
    /// `CHARACTER_LENGTH` (`CHARACTER_LENGTH`)
    CharacterLength,
    /// `CHARACTERISTICS` (`CHARACTERISTICS`)
    Characteristics,
    /// `CLOSE` (`CLOSE`)
    Close,
    /// `COALESCE` (`COALESCE`)
    Coalesce,
    /// `COLLECT_LIST` (`COLLECT_LIST`)
    CollectList,
    /// `COMMIT` (`COMMIT`)
    Commit,
    /// `COPY` (`COPY`)
    Copy,
    /// `COS` (`COS`)
    Cos,
    /// `COSH` (`COSH`)
    Cosh,
    /// `COT` (`COT`)
    Cot,
    /// `COUNT` (`COUNT`)
    Count,
    /// `CREATE` (`CREATE`)
    Create,
    /// `CURRENT_DATE` (`CURRENT_DATE`)
    CurrentDate,
    /// `CURRENT_GRAPH` (`CURRENT_GRAPH`)
    CurrentGraph,
    /// `CURRENT_PROPERTY_GRAPH` (`CURRENT_PROPERTY_GRAPH`)
    CurrentPropertyGraph,
    /// `CURRENT_SCHEMA` (`CURRENT_SCHEMA`)
    CurrentSchema,
    /// `CURRENT_TIME` (`CURRENT_TIME`)
    CurrentTime,
    /// `CURRENT_TIMESTAMP` (`CURRENT_TIMESTAMP`)
    CurrentTimestamp,
    /// `DATE` (`DATE`)
    Date,
    /// `DATETIME` (`DATETIME`)
    Datetime,
    /// `DAY` (`DAY`)
    Day,
    /// `DEC` (`DEC`)
    Dec,
    /// `DECIMAL` (`DECIMAL`)
    Decimal,
    /// `DEGREES` (`DEGREES`)
    Degrees,
    /// `DELETE` (`DELETE`)
    Delete,
    /// `DESC` (`DESC`)
    Desc,
    /// `DESCENDING` (`DESCENDING`)
    Descending,
    /// `DETACH` (`DETACH`)
    Detach,
    /// `DISTINCT` (`DISTINCT`)
    Distinct,
    /// `DOUBLE` (`DOUBLE`)
    Double,
    /// `DROP` (`DROP`)
    Drop,
    /// `DURATION` (`DURATION`)
    Duration,
    /// `DURATION_BETWEEN` (`DURATION_BETWEEN`)
    DurationBetween,
    /// `ELEMENT_ID` (`ELEMENT_ID`)
    ElementId,
    /// `ELSE` (`ELSE`)
    Else,
    /// `END` (`END`)
    End,
    /// `EXCEPT` (`EXCEPT`)
    Except,
    /// `EXISTS` (`EXISTS`)
    Exists,
    /// `EXP` (`EXP`)
    Exp,
    /// `FILTER` (`FILTER`)
    Filter,
    /// `FINISH` (`FINISH`)
    Finish,
    /// `FLOAT` (`FLOAT`)
    Float,
    /// `FLOAT16` (`FLOAT16`)
    Float16,
    /// `FLOAT32` (`FLOAT32`)
    Float32,
    /// `FLOAT64` (`FLOAT64`)
    Float64,
    /// `FLOAT128` (`FLOAT128`)
    Float128,
    /// `FLOAT256` (`FLOAT256`)
    Float256,
    /// `FLOOR` (`FLOOR`)
    Floor,
    /// `FOR` (`FOR`)
    For,
    /// `FROM` (`FROM`)
    From,
    /// `GROUP` (`GROUP`)
    Group,
    /// `HAVING` (`HAVING`)
    Having,
    /// `HOME_GRAPH` (`HOME_GRAPH`)
    HomeGraph,
    /// `HOME_PROPERTY_GRAPH` (`HOME_PROPERTY_GRAPH`)
    HomePropertyGraph,
    /// `HOME_SCHEMA` (`HOME_SCHEMA`)
    HomeSchema,
    /// `HOUR` (`HOUR`)
    Hour,
    /// `IF` (`IF`)
    If,
    /// `IN` (`IN`)
    In,
    /// `INSERT` (`INSERT`)
    Insert,
    /// `INT` (`INT`)
    Int,
    /// `INTEGER` (`INTEGER`)
    Integer,
    /// `INT8` (`INT8`)
    Int8,
    /// `INTEGER8` (`INTEGER8`)
    Integer8,
    /// `INT16` (`INT16`)
    Int16,
    /// `INTEGER16` (`INTEGER16`)
    Integer16,
    /// `INT32` (`INT32`)
    Int32,
    /// `INTEGER32` (`INTEGER32`)
    Integer32,
    /// `INT64` (`INT64`)
    Int64,
    /// `INTEGER64` (`INTEGER64`)
    Integer64,
    /// `INT128` (`INT128`)
    Int128,
    /// `INTEGER128` (`INTEGER128`)
    Integer128,
    /// `INT256` (`INT256`)
    Int256,
    /// `INTEGER256` (`INTEGER256`)
    Integer256,
    /// `INTERSECT` (`INTERSECT`)
    Intersect,
    /// `INTERVAL` (`INTERVAL`)
    Interval,
    /// `IS` (`IS`)
    Is,
    /// `LEADING` (`LEADING`)
    Leading,
    /// `LEFT` (`LEFT`)
    Left,
    /// `LET` (`LET`)
    Let,
    /// `LIKE` (`LIKE`)
    Like,
    /// `LIMIT` (`LIMIT`)
    Limit,
    /// `LIST` (`LIST`)
    List,
    /// `LN` (`LN`)
    Ln,
    /// `LOCAL` (`LOCAL`)
    Local,
    /// `LOCAL_DATETIME` (`LOCAL_DATETIME`)
    LocalDatetime,
    /// `LOCAL_TIME` (`LOCAL_TIME`)
    LocalTime,
    /// `LOCAL_TIMESTAMP` (`LOCAL_TIMESTAMP`)
    LocalTimestamp,
    /// `LOG` (`LOG_KW`)
    Log,
    /// `LOG10` (`LOG10`)
    Log10,
    /// `LOWER` (`LOWER`)
    Lower,
    /// `LTRIM` (`LTRIM`)
    Ltrim,
    /// `MATCH` (`MATCH`)
    Match,
    /// `MAX` (`MAX`)
    Max,
    /// `MIN` (`MIN`)
    Min,
    /// `MINUTE` (`MINUTE`)
    Minute,
    /// `MOD` (`MOD`)
    Mod,
    /// `MONTH` (`MONTH`)
    Month,
    /// `NEXT` (`NEXT`)
    Next,
    /// `NODETACH` (`NODETACH`)
    Nodetach,
    /// `NORMALIZE` (`NORMALIZE`)
    Normalize,
    /// `NOT` (`NOT`)
    Not,
    /// `NOTHING` (`NOTHING`)
    Nothing,
    /// `NULL` (`NULL_KW`)
    Null,
    /// `NULLS` (`NULLS`)
    Nulls,
    /// `NULLIF` (`NULLIF`)
    Nullif,
    /// `OCTET_LENGTH` (`OCTET_LENGTH`)
    OctetLength,
    /// `OF` (`OF`)
    Of,
    /// `OFFSET` (`OFFSET`)
    Offset,
    /// `OPTIONAL` (`OPTIONAL`)
    Optional,
    /// `OR` (`OR`)
    Or,
    /// `ORDER` (`ORDER`)
    Order,
    /// `OTHERWISE` (`OTHERWISE`)
    Otherwise,
    /// `PARAMETER` (`PARAMETER`)
    Parameter,
    /// `PARAMETERS` (`PARAMETERS`)
    Parameters,
    /// `PATH` (`PATH`)
    Path,
    /// `PATH_LENGTH` (`PATH_LENGTH`)
    PathLength,
    /// `PATHS` (`PATHS`)
    Paths,
    /// `PERCENTILE_CONT` (`PERCENTILE_CONT`)
    PercentileCont,
    /// `PERCENTILE_DISC` (`PERCENTILE_DISC`)
    PercentileDisc,
    /// `POWER` (`POWER`)
    Power,
    /// `PRECISION` (`PRECISION`)
    Precision,
    /// `PROPERTY_EXISTS` (`PROPERTY_EXISTS`)
    PropertyExists,
    /// `RADIANS` (`RADIANS`)
    Radians,
    /// `REAL` (`REAL`)
    Real,
    /// `RECORD` (`RECORD`)
    Record,
    /// `REMOVE` (`REMOVE`)
    Remove,
    /// `REPLACE` (`REPLACE`)
    Replace,
    /// `RESET` (`RESET`)
    Reset,
    /// `RETURN` (`RETURN`)
    Return,
    /// `RIGHT` (`RIGHT`)
    Right,
    /// `ROLLBACK` (`ROLLBACK`)
    Rollback,
    /// `RTRIM` (`RTRIM`)
    Rtrim,
    /// `SAME` (`SAME`)
    Same,
    /// `SCHEMA` (`SCHEMA`)
    Schema,
    /// `SECOND` (`SECOND`)
    Second,
    /// `SELECT` (`SELECT`)
    Select,
    /// `SESSION` (`SESSION`)
    Session,
    /// `SESSION_USER` (`SESSION_USER`)
    SessionUser,
    /// `SET` (`SET`)
    Set,
    /// `SIGNED` (`SIGNED`)
    Signed,
    /// `SIN` (`SIN`)
    Sin,
    /// `SINH` (`SINH`)
    Sinh,
    /// `SIZE` (`SIZE`)
    Size,
    /// `SKIP` (`SKIP_RESERVED_WORD`)
    Skip,
    /// `SMALL` (`SMALL`)
    Small,
    /// `SMALLINT` (`SMALLINT`)
    Smallint,
    /// `SQRT` (`SQRT`)
    Sqrt,
    /// `START` (`START`)
    Start,
    /// `STDDEV_POP` (`STDDEV_POP`)
    StddevPop,
    /// `STDDEV_SAMP` (`STDDEV_SAMP`)
    StddevSamp,
    /// `STRING` (`STRING`)
    String,
    /// `SUM` (`SUM`)
    Sum,
    /// `TAN` (`TAN`)
    Tan,
    /// `TANH` (`TANH`)
    Tanh,
    /// `THEN` (`THEN`)
    Then,
    /// `TIME` (`TIME`)
    Time,
    /// `TIMESTAMP` (`TIMESTAMP`)
    Timestamp,
    /// `TRAILING` (`TRAILING`)
    Trailing,
    /// `TRIM` (`TRIM`)
    Trim,
    /// `TYPED` (`TYPED`)
    Typed,
    /// `UBIGINT` (`UBIGINT`)
    Ubigint,
    /// `UINT` (`UINT`)
    Uint,
    /// `UINT8` (`UINT8`)
    Uint8,
    /// `UINT16` (`UINT16`)
    Uint16,
    /// `UINT32` (`UINT32`)
    Uint32,
    /// `UINT64` (`UINT64`)
    Uint64,
    /// `UINT128` (`UINT128`)
    Uint128,
    /// `UINT256` (`UINT256`)
    Uint256,
    /// `UNION` (`UNION`)
    Union,
    /// `UNSIGNED` (`UNSIGNED`)
    Unsigned,
    /// `UPPER` (`UPPER`)
    Upper,
    /// `USE` (`USE`)
    Use,
    /// `USMALLINT` (`USMALLINT`)
    Usmallint,
    /// `VALUE` (`VALUE`)
    Value,
    /// `VARBINARY` (`VARBINARY`)
    Varbinary,
    /// `VARCHAR` (`VARCHAR`)
    Varchar,
    /// `VARIABLE` (`VARIABLE`)
    Variable,
    /// `WHEN` (`WHEN`)
    When,
    /// `WHERE` (`WHERE`)
    Where,
    /// `WITH` (`WITH`)
    With,
    /// `XOR` (`XOR`)
    Xor,
    /// `YEAR` (`YEAR`)
    Year,
    /// `YIELD` (`YIELD`)
    Yield,
    /// `ZONED` (`ZONED`)
    Zoned,
    /// `ZONED_DATETIME` (`ZONED_DATETIME`)
    ZonedDatetime,
    /// `ZONED_TIME` (`ZONED_TIME`)
    ZonedTime,
    /// `ABSTRACT` (`ABSTRACT`)
    Abstract,
    /// `AGGREGATE` (`AGGREGATE`)
    Aggregate,
    /// `AGGREGATES` (`AGGREGATES`)
    Aggregates,
    /// `ALTER` (`ALTER`)
    Alter,
    /// `CATALOG` (`CATALOG`)
    Catalog,
    /// `CLEAR` (`CLEAR`)
    Clear,
    /// `CLONE` (`CLONE`)
    Clone,
    /// `CONSTRAINT` (`CONSTRAINT`)
    Constraint,
    /// `CURRENT_ROLE` (`CURRENT_ROLE`)
    CurrentRole,
    /// `CURRENT_USER` (`CURRENT_USER`)
    CurrentUser,
    /// `DATA` (`DATA`)
    Data,
    /// `DIRECTORY` (`DIRECTORY`)
    Directory,
    /// `DRYRUN` (`DRYRUN`)
    Dryrun,
    /// `EXACT` (`EXACT`)
    Exact,
    /// `EXISTING` (`EXISTING`)
    Existing,
    /// `FUNCTION` (`FUNCTION`)
    Function,
    /// `GQLSTATUS` (`GQLSTATUS`)
    Gqlstatus,
    /// `GRANT` (`GRANT`)
    Grant,
    /// `INSTANT` (`INSTANT`)
    Instant,
    /// `INFINITY` (`INFINITY_KW`)
    Infinity,
    /// `NUMBER` (`NUMBER`)
    Number,
    /// `NUMERIC` (`NUMERIC`)
    Numeric,
    /// `ON` (`ON`)
    On,
    /// `OPEN` (`OPEN`)
    Open,
    /// `PARTITION` (`PARTITION`)
    Partition,
    /// `PROCEDURE` (`PROCEDURE`)
    Procedure,
    /// `PRODUCT` (`PRODUCT`)
    Product,
    /// `PROJECT` (`PROJECT`)
    Project,
    /// `QUERY` (`QUERY`)
    Query,
    /// `RECORDS` (`RECORDS`)
    Records,
    /// `REFERENCE` (`REFERENCE`)
    Reference,
    /// `RENAME` (`RENAME`)
    Rename,
    /// `REVOKE` (`REVOKE`)
    Revoke,
    /// `SUBSTRING` (`SUBSTRING`)
    Substring,
    /// `SYSTEM_USER` (`SYSTEM_USER`)
    SystemUser,
    /// `TEMPORAL` (`TEMPORAL`)
    Temporal,
    /// `UNIQUE` (`UNIQUE`)
    Unique,
    /// `UNIT` (`UNIT`)
    Unit,
    /// `VALUES` (`VALUES`)
    Values,
    /// `ACYCLIC` (`ACYCLIC`)
    Acyclic,
    /// `BINDING` (`BINDING`)
    Binding,
    /// `BINDINGS` (`BINDINGS`)
    Bindings,
    /// `CONNECTING` (`CONNECTING`)
    Connecting,
    /// `DESTINATION` (`DESTINATION`)
    Destination,
    /// `DIFFERENT` (`DIFFERENT`)
    Different,
    /// `DIRECTED` (`DIRECTED`)
    Directed,
    /// `EDGE` (`EDGE`)
    Edge,
    /// `EDGES` (`EDGES`)
    Edges,
    /// `ELEMENT` (`ELEMENT`)
    Element,
    /// `ELEMENTS` (`ELEMENTS`)
    Elements,
    /// `FIRST` (`FIRST`)
    First,
    /// `GRAPH` (`GRAPH`)
    Graph,
    /// `GROUPS` (`GROUPS`)
    Groups,
    /// `KEEP` (`KEEP`)
    Keep,
    /// `LABEL` (`LABEL`)
    Label,
    /// `LABELED` (`LABELED`)
    Labeled,
    /// `LABELS` (`LABELS`)
    Labels,
    /// `LAST` (`LAST`)
    Last,
    /// `NFC` (`NFC`)
    Nfc,
    /// `NFD` (`NFD`)
    Nfd,
    /// `NFKC` (`NFKC`)
    Nfkc,
    /// `NFKD` (`NFKD`)
    Nfkd,
    /// `NO` (`NO`)
    No,
    /// `NODE` (`NODE`)
    Node,
    /// `NORMALIZED` (`NORMALIZED`)
    Normalized,
    /// `ONLY` (`ONLY`)
    Only,
    /// `ORDINALITY` (`ORDINALITY`)
    Ordinality,
    /// `PROPERTY` (`PROPERTY`)
    Property,
    /// `READ` (`READ`)
    Read,
    /// `RELATIONSHIP` (`RELATIONSHIP`)
    Relationship,
    /// `RELATIONSHIPS` (`RELATIONSHIPS`)
    Relationships,
    /// `REPEATABLE` (`REPEATABLE`)
    Repeatable,
    /// `SHORTEST` (`SHORTEST`)
    Shortest,
    /// `SIMPLE` (`SIMPLE`)
    Simple,
    /// `SOURCE` (`SOURCE`)
    Source,
    /// `TABLE` (`TABLE`)
    Table,
    /// `TO` (`TO`)
    To,
    /// `TRAIL` (`TRAIL`)
    Trail,
    /// `TRANSACTION` (`TRANSACTION`)
    Transaction,
    /// `TYPE` (`TYPE`)
    Type,
    /// `UNDIRECTED` (`UNDIRECTED`)
    Undirected,
    /// `VERTEX` (`VERTEX`)
    Vertex,
    /// `WALK` (`WALK`)
    Walk,
    /// `WITHOUT` (`WITHOUT`)
    Without,
    /// `WRITE` (`WRITE`)
    Write,
    /// `ZONE` (`ZONE`)
    Zone,
}

/// How a keyword may be used (`GQL.g4` sections of 21.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reservation {
    /// Never an identifier.
    Reserved,
    /// Reserved for a later edition; never an identifier.
    PreReserved,
    /// An identifier where the grammar allows `nonReservedWords`.
    NonReserved,
}

/// Every keyword: its text, the keyword and its reservation, sorted
/// by text for a binary search.
pub(crate) static KEYWORDS: &[(&str, Keyword, Reservation)] = &[
    ("ABS", Keyword::Abs, Reservation::Reserved),
    ("ABSTRACT", Keyword::Abstract, Reservation::PreReserved),
    ("ACOS", Keyword::Acos, Reservation::Reserved),
    ("ACYCLIC", Keyword::Acyclic, Reservation::NonReserved),
    ("AGGREGATE", Keyword::Aggregate, Reservation::PreReserved),
    ("AGGREGATES", Keyword::Aggregates, Reservation::PreReserved),
    ("ALL", Keyword::All, Reservation::Reserved),
    (
        "ALL_DIFFERENT",
        Keyword::AllDifferent,
        Reservation::Reserved,
    ),
    ("ALTER", Keyword::Alter, Reservation::PreReserved),
    ("AND", Keyword::And, Reservation::Reserved),
    ("ANY", Keyword::Any, Reservation::Reserved),
    ("ARRAY", Keyword::Array, Reservation::Reserved),
    ("AS", Keyword::As, Reservation::Reserved),
    ("ASC", Keyword::Asc, Reservation::Reserved),
    ("ASCENDING", Keyword::Ascending, Reservation::Reserved),
    ("ASIN", Keyword::Asin, Reservation::Reserved),
    ("AT", Keyword::At, Reservation::Reserved),
    ("ATAN", Keyword::Atan, Reservation::Reserved),
    ("AVG", Keyword::Avg, Reservation::Reserved),
    ("BIG", Keyword::Big, Reservation::Reserved),
    ("BIGINT", Keyword::Bigint, Reservation::Reserved),
    ("BINARY", Keyword::Binary, Reservation::Reserved),
    ("BINDING", Keyword::Binding, Reservation::NonReserved),
    ("BINDINGS", Keyword::Bindings, Reservation::NonReserved),
    ("BOOL", Keyword::Bool, Reservation::Reserved),
    ("BOOLEAN", Keyword::Boolean, Reservation::Reserved),
    ("BOTH", Keyword::Both, Reservation::Reserved),
    ("BTRIM", Keyword::Btrim, Reservation::Reserved),
    ("BY", Keyword::By, Reservation::Reserved),
    ("BYTES", Keyword::Bytes, Reservation::Reserved),
    ("BYTE_LENGTH", Keyword::ByteLength, Reservation::Reserved),
    ("CALL", Keyword::Call, Reservation::Reserved),
    ("CARDINALITY", Keyword::Cardinality, Reservation::Reserved),
    ("CASE", Keyword::Case, Reservation::Reserved),
    ("CAST", Keyword::Cast, Reservation::Reserved),
    ("CATALOG", Keyword::Catalog, Reservation::PreReserved),
    ("CEIL", Keyword::Ceil, Reservation::Reserved),
    ("CEILING", Keyword::Ceiling, Reservation::Reserved),
    ("CHAR", Keyword::Char, Reservation::Reserved),
    (
        "CHARACTERISTICS",
        Keyword::Characteristics,
        Reservation::Reserved,
    ),
    (
        "CHARACTER_LENGTH",
        Keyword::CharacterLength,
        Reservation::Reserved,
    ),
    ("CHAR_LENGTH", Keyword::CharLength, Reservation::Reserved),
    ("CLEAR", Keyword::Clear, Reservation::PreReserved),
    ("CLONE", Keyword::Clone, Reservation::PreReserved),
    ("CLOSE", Keyword::Close, Reservation::Reserved),
    ("COALESCE", Keyword::Coalesce, Reservation::Reserved),
    ("COLLECT_LIST", Keyword::CollectList, Reservation::Reserved),
    ("COMMIT", Keyword::Commit, Reservation::Reserved),
    ("CONNECTING", Keyword::Connecting, Reservation::NonReserved),
    ("CONSTRAINT", Keyword::Constraint, Reservation::PreReserved),
    ("COPY", Keyword::Copy, Reservation::Reserved),
    ("COS", Keyword::Cos, Reservation::Reserved),
    ("COSH", Keyword::Cosh, Reservation::Reserved),
    ("COT", Keyword::Cot, Reservation::Reserved),
    ("COUNT", Keyword::Count, Reservation::Reserved),
    ("CREATE", Keyword::Create, Reservation::Reserved),
    ("CURRENT_DATE", Keyword::CurrentDate, Reservation::Reserved),
    (
        "CURRENT_GRAPH",
        Keyword::CurrentGraph,
        Reservation::Reserved,
    ),
    (
        "CURRENT_PROPERTY_GRAPH",
        Keyword::CurrentPropertyGraph,
        Reservation::Reserved,
    ),
    (
        "CURRENT_ROLE",
        Keyword::CurrentRole,
        Reservation::PreReserved,
    ),
    (
        "CURRENT_SCHEMA",
        Keyword::CurrentSchema,
        Reservation::Reserved,
    ),
    ("CURRENT_TIME", Keyword::CurrentTime, Reservation::Reserved),
    (
        "CURRENT_TIMESTAMP",
        Keyword::CurrentTimestamp,
        Reservation::Reserved,
    ),
    (
        "CURRENT_USER",
        Keyword::CurrentUser,
        Reservation::PreReserved,
    ),
    ("DATA", Keyword::Data, Reservation::PreReserved),
    ("DATE", Keyword::Date, Reservation::Reserved),
    ("DATETIME", Keyword::Datetime, Reservation::Reserved),
    ("DAY", Keyword::Day, Reservation::Reserved),
    ("DEC", Keyword::Dec, Reservation::Reserved),
    ("DECIMAL", Keyword::Decimal, Reservation::Reserved),
    ("DEGREES", Keyword::Degrees, Reservation::Reserved),
    ("DELETE", Keyword::Delete, Reservation::Reserved),
    ("DESC", Keyword::Desc, Reservation::Reserved),
    ("DESCENDING", Keyword::Descending, Reservation::Reserved),
    (
        "DESTINATION",
        Keyword::Destination,
        Reservation::NonReserved,
    ),
    ("DETACH", Keyword::Detach, Reservation::Reserved),
    ("DIFFERENT", Keyword::Different, Reservation::NonReserved),
    ("DIRECTED", Keyword::Directed, Reservation::NonReserved),
    ("DIRECTORY", Keyword::Directory, Reservation::PreReserved),
    ("DISTINCT", Keyword::Distinct, Reservation::Reserved),
    ("DOUBLE", Keyword::Double, Reservation::Reserved),
    ("DROP", Keyword::Drop, Reservation::Reserved),
    ("DRYRUN", Keyword::Dryrun, Reservation::PreReserved),
    ("DURATION", Keyword::Duration, Reservation::Reserved),
    (
        "DURATION_BETWEEN",
        Keyword::DurationBetween,
        Reservation::Reserved,
    ),
    ("EDGE", Keyword::Edge, Reservation::NonReserved),
    ("EDGES", Keyword::Edges, Reservation::NonReserved),
    ("ELEMENT", Keyword::Element, Reservation::NonReserved),
    ("ELEMENTS", Keyword::Elements, Reservation::NonReserved),
    ("ELEMENT_ID", Keyword::ElementId, Reservation::Reserved),
    ("ELSE", Keyword::Else, Reservation::Reserved),
    ("END", Keyword::End, Reservation::Reserved),
    ("EXACT", Keyword::Exact, Reservation::PreReserved),
    ("EXCEPT", Keyword::Except, Reservation::Reserved),
    ("EXISTING", Keyword::Existing, Reservation::PreReserved),
    ("EXISTS", Keyword::Exists, Reservation::Reserved),
    ("EXP", Keyword::Exp, Reservation::Reserved),
    ("FILTER", Keyword::Filter, Reservation::Reserved),
    ("FINISH", Keyword::Finish, Reservation::Reserved),
    ("FIRST", Keyword::First, Reservation::NonReserved),
    ("FLOAT", Keyword::Float, Reservation::Reserved),
    ("FLOAT128", Keyword::Float128, Reservation::Reserved),
    ("FLOAT16", Keyword::Float16, Reservation::Reserved),
    ("FLOAT256", Keyword::Float256, Reservation::Reserved),
    ("FLOAT32", Keyword::Float32, Reservation::Reserved),
    ("FLOAT64", Keyword::Float64, Reservation::Reserved),
    ("FLOOR", Keyword::Floor, Reservation::Reserved),
    ("FOR", Keyword::For, Reservation::Reserved),
    ("FROM", Keyword::From, Reservation::Reserved),
    ("FUNCTION", Keyword::Function, Reservation::PreReserved),
    ("GQLSTATUS", Keyword::Gqlstatus, Reservation::PreReserved),
    ("GRANT", Keyword::Grant, Reservation::PreReserved),
    ("GRAPH", Keyword::Graph, Reservation::NonReserved),
    ("GROUP", Keyword::Group, Reservation::Reserved),
    ("GROUPS", Keyword::Groups, Reservation::NonReserved),
    ("HAVING", Keyword::Having, Reservation::Reserved),
    ("HOME_GRAPH", Keyword::HomeGraph, Reservation::Reserved),
    (
        "HOME_PROPERTY_GRAPH",
        Keyword::HomePropertyGraph,
        Reservation::Reserved,
    ),
    ("HOME_SCHEMA", Keyword::HomeSchema, Reservation::Reserved),
    ("HOUR", Keyword::Hour, Reservation::Reserved),
    ("IF", Keyword::If, Reservation::Reserved),
    ("IN", Keyword::In, Reservation::Reserved),
    ("INFINITY", Keyword::Infinity, Reservation::PreReserved),
    ("INSERT", Keyword::Insert, Reservation::Reserved),
    ("INSTANT", Keyword::Instant, Reservation::PreReserved),
    ("INT", Keyword::Int, Reservation::Reserved),
    ("INT128", Keyword::Int128, Reservation::Reserved),
    ("INT16", Keyword::Int16, Reservation::Reserved),
    ("INT256", Keyword::Int256, Reservation::Reserved),
    ("INT32", Keyword::Int32, Reservation::Reserved),
    ("INT64", Keyword::Int64, Reservation::Reserved),
    ("INT8", Keyword::Int8, Reservation::Reserved),
    ("INTEGER", Keyword::Integer, Reservation::Reserved),
    ("INTEGER128", Keyword::Integer128, Reservation::Reserved),
    ("INTEGER16", Keyword::Integer16, Reservation::Reserved),
    ("INTEGER256", Keyword::Integer256, Reservation::Reserved),
    ("INTEGER32", Keyword::Integer32, Reservation::Reserved),
    ("INTEGER64", Keyword::Integer64, Reservation::Reserved),
    ("INTEGER8", Keyword::Integer8, Reservation::Reserved),
    ("INTERSECT", Keyword::Intersect, Reservation::Reserved),
    ("INTERVAL", Keyword::Interval, Reservation::Reserved),
    ("IS", Keyword::Is, Reservation::Reserved),
    ("KEEP", Keyword::Keep, Reservation::NonReserved),
    ("LABEL", Keyword::Label, Reservation::NonReserved),
    ("LABELED", Keyword::Labeled, Reservation::NonReserved),
    ("LABELS", Keyword::Labels, Reservation::NonReserved),
    ("LAST", Keyword::Last, Reservation::NonReserved),
    ("LEADING", Keyword::Leading, Reservation::Reserved),
    ("LEFT", Keyword::Left, Reservation::Reserved),
    ("LET", Keyword::Let, Reservation::Reserved),
    ("LIKE", Keyword::Like, Reservation::Reserved),
    ("LIMIT", Keyword::Limit, Reservation::Reserved),
    ("LIST", Keyword::List, Reservation::Reserved),
    ("LN", Keyword::Ln, Reservation::Reserved),
    ("LOCAL", Keyword::Local, Reservation::Reserved),
    (
        "LOCAL_DATETIME",
        Keyword::LocalDatetime,
        Reservation::Reserved,
    ),
    ("LOCAL_TIME", Keyword::LocalTime, Reservation::Reserved),
    (
        "LOCAL_TIMESTAMP",
        Keyword::LocalTimestamp,
        Reservation::Reserved,
    ),
    ("LOG", Keyword::Log, Reservation::Reserved),
    ("LOG10", Keyword::Log10, Reservation::Reserved),
    ("LOWER", Keyword::Lower, Reservation::Reserved),
    ("LTRIM", Keyword::Ltrim, Reservation::Reserved),
    ("MATCH", Keyword::Match, Reservation::Reserved),
    ("MAX", Keyword::Max, Reservation::Reserved),
    ("MIN", Keyword::Min, Reservation::Reserved),
    ("MINUTE", Keyword::Minute, Reservation::Reserved),
    ("MOD", Keyword::Mod, Reservation::Reserved),
    ("MONTH", Keyword::Month, Reservation::Reserved),
    ("NEXT", Keyword::Next, Reservation::Reserved),
    ("NFC", Keyword::Nfc, Reservation::NonReserved),
    ("NFD", Keyword::Nfd, Reservation::NonReserved),
    ("NFKC", Keyword::Nfkc, Reservation::NonReserved),
    ("NFKD", Keyword::Nfkd, Reservation::NonReserved),
    ("NO", Keyword::No, Reservation::NonReserved),
    ("NODE", Keyword::Node, Reservation::NonReserved),
    ("NODETACH", Keyword::Nodetach, Reservation::Reserved),
    ("NORMALIZE", Keyword::Normalize, Reservation::Reserved),
    ("NORMALIZED", Keyword::Normalized, Reservation::NonReserved),
    ("NOT", Keyword::Not, Reservation::Reserved),
    ("NOTHING", Keyword::Nothing, Reservation::Reserved),
    ("NULL", Keyword::Null, Reservation::Reserved),
    ("NULLIF", Keyword::Nullif, Reservation::Reserved),
    ("NULLS", Keyword::Nulls, Reservation::Reserved),
    ("NUMBER", Keyword::Number, Reservation::PreReserved),
    ("NUMERIC", Keyword::Numeric, Reservation::PreReserved),
    ("OCTET_LENGTH", Keyword::OctetLength, Reservation::Reserved),
    ("OF", Keyword::Of, Reservation::Reserved),
    ("OFFSET", Keyword::Offset, Reservation::Reserved),
    ("ON", Keyword::On, Reservation::PreReserved),
    ("ONLY", Keyword::Only, Reservation::NonReserved),
    ("OPEN", Keyword::Open, Reservation::PreReserved),
    ("OPTIONAL", Keyword::Optional, Reservation::Reserved),
    ("OR", Keyword::Or, Reservation::Reserved),
    ("ORDER", Keyword::Order, Reservation::Reserved),
    ("ORDINALITY", Keyword::Ordinality, Reservation::NonReserved),
    ("OTHERWISE", Keyword::Otherwise, Reservation::Reserved),
    ("PARAMETER", Keyword::Parameter, Reservation::Reserved),
    ("PARAMETERS", Keyword::Parameters, Reservation::Reserved),
    ("PARTITION", Keyword::Partition, Reservation::PreReserved),
    ("PATH", Keyword::Path, Reservation::Reserved),
    ("PATHS", Keyword::Paths, Reservation::Reserved),
    ("PATH_LENGTH", Keyword::PathLength, Reservation::Reserved),
    (
        "PERCENTILE_CONT",
        Keyword::PercentileCont,
        Reservation::Reserved,
    ),
    (
        "PERCENTILE_DISC",
        Keyword::PercentileDisc,
        Reservation::Reserved,
    ),
    ("POWER", Keyword::Power, Reservation::Reserved),
    ("PRECISION", Keyword::Precision, Reservation::Reserved),
    ("PROCEDURE", Keyword::Procedure, Reservation::PreReserved),
    ("PRODUCT", Keyword::Product, Reservation::PreReserved),
    ("PROJECT", Keyword::Project, Reservation::PreReserved),
    ("PROPERTY", Keyword::Property, Reservation::NonReserved),
    (
        "PROPERTY_EXISTS",
        Keyword::PropertyExists,
        Reservation::Reserved,
    ),
    ("QUERY", Keyword::Query, Reservation::PreReserved),
    ("RADIANS", Keyword::Radians, Reservation::Reserved),
    ("READ", Keyword::Read, Reservation::NonReserved),
    ("REAL", Keyword::Real, Reservation::Reserved),
    ("RECORD", Keyword::Record, Reservation::Reserved),
    ("RECORDS", Keyword::Records, Reservation::PreReserved),
    ("REFERENCE", Keyword::Reference, Reservation::PreReserved),
    (
        "RELATIONSHIP",
        Keyword::Relationship,
        Reservation::NonReserved,
    ),
    (
        "RELATIONSHIPS",
        Keyword::Relationships,
        Reservation::NonReserved,
    ),
    ("REMOVE", Keyword::Remove, Reservation::Reserved),
    ("RENAME", Keyword::Rename, Reservation::PreReserved),
    ("REPEATABLE", Keyword::Repeatable, Reservation::NonReserved),
    ("REPLACE", Keyword::Replace, Reservation::Reserved),
    ("RESET", Keyword::Reset, Reservation::Reserved),
    ("RETURN", Keyword::Return, Reservation::Reserved),
    ("REVOKE", Keyword::Revoke, Reservation::PreReserved),
    ("RIGHT", Keyword::Right, Reservation::Reserved),
    ("ROLLBACK", Keyword::Rollback, Reservation::Reserved),
    ("RTRIM", Keyword::Rtrim, Reservation::Reserved),
    ("SAME", Keyword::Same, Reservation::Reserved),
    ("SCHEMA", Keyword::Schema, Reservation::Reserved),
    ("SECOND", Keyword::Second, Reservation::Reserved),
    ("SELECT", Keyword::Select, Reservation::Reserved),
    ("SESSION", Keyword::Session, Reservation::Reserved),
    ("SESSION_USER", Keyword::SessionUser, Reservation::Reserved),
    ("SET", Keyword::Set, Reservation::Reserved),
    ("SHORTEST", Keyword::Shortest, Reservation::NonReserved),
    ("SIGNED", Keyword::Signed, Reservation::Reserved),
    ("SIMPLE", Keyword::Simple, Reservation::NonReserved),
    ("SIN", Keyword::Sin, Reservation::Reserved),
    ("SINH", Keyword::Sinh, Reservation::Reserved),
    ("SIZE", Keyword::Size, Reservation::Reserved),
    ("SKIP", Keyword::Skip, Reservation::Reserved),
    ("SMALL", Keyword::Small, Reservation::Reserved),
    ("SMALLINT", Keyword::Smallint, Reservation::Reserved),
    ("SOURCE", Keyword::Source, Reservation::NonReserved),
    ("SQRT", Keyword::Sqrt, Reservation::Reserved),
    ("START", Keyword::Start, Reservation::Reserved),
    ("STDDEV_POP", Keyword::StddevPop, Reservation::Reserved),
    ("STDDEV_SAMP", Keyword::StddevSamp, Reservation::Reserved),
    ("STRING", Keyword::String, Reservation::Reserved),
    ("SUBSTRING", Keyword::Substring, Reservation::PreReserved),
    ("SUM", Keyword::Sum, Reservation::Reserved),
    ("SYSTEM_USER", Keyword::SystemUser, Reservation::PreReserved),
    ("TABLE", Keyword::Table, Reservation::NonReserved),
    ("TAN", Keyword::Tan, Reservation::Reserved),
    ("TANH", Keyword::Tanh, Reservation::Reserved),
    ("TEMPORAL", Keyword::Temporal, Reservation::PreReserved),
    ("THEN", Keyword::Then, Reservation::Reserved),
    ("TIME", Keyword::Time, Reservation::Reserved),
    ("TIMESTAMP", Keyword::Timestamp, Reservation::Reserved),
    ("TO", Keyword::To, Reservation::NonReserved),
    ("TRAIL", Keyword::Trail, Reservation::NonReserved),
    ("TRAILING", Keyword::Trailing, Reservation::Reserved),
    (
        "TRANSACTION",
        Keyword::Transaction,
        Reservation::NonReserved,
    ),
    ("TRIM", Keyword::Trim, Reservation::Reserved),
    ("TYPE", Keyword::Type, Reservation::NonReserved),
    ("TYPED", Keyword::Typed, Reservation::Reserved),
    ("UBIGINT", Keyword::Ubigint, Reservation::Reserved),
    ("UINT", Keyword::Uint, Reservation::Reserved),
    ("UINT128", Keyword::Uint128, Reservation::Reserved),
    ("UINT16", Keyword::Uint16, Reservation::Reserved),
    ("UINT256", Keyword::Uint256, Reservation::Reserved),
    ("UINT32", Keyword::Uint32, Reservation::Reserved),
    ("UINT64", Keyword::Uint64, Reservation::Reserved),
    ("UINT8", Keyword::Uint8, Reservation::Reserved),
    ("UNDIRECTED", Keyword::Undirected, Reservation::NonReserved),
    ("UNION", Keyword::Union, Reservation::Reserved),
    ("UNIQUE", Keyword::Unique, Reservation::PreReserved),
    ("UNIT", Keyword::Unit, Reservation::PreReserved),
    ("UNSIGNED", Keyword::Unsigned, Reservation::Reserved),
    ("UPPER", Keyword::Upper, Reservation::Reserved),
    ("USE", Keyword::Use, Reservation::Reserved),
    ("USMALLINT", Keyword::Usmallint, Reservation::Reserved),
    ("VALUE", Keyword::Value, Reservation::Reserved),
    ("VALUES", Keyword::Values, Reservation::PreReserved),
    ("VARBINARY", Keyword::Varbinary, Reservation::Reserved),
    ("VARCHAR", Keyword::Varchar, Reservation::Reserved),
    ("VARIABLE", Keyword::Variable, Reservation::Reserved),
    ("VERTEX", Keyword::Vertex, Reservation::NonReserved),
    ("WALK", Keyword::Walk, Reservation::NonReserved),
    ("WHEN", Keyword::When, Reservation::Reserved),
    ("WHERE", Keyword::Where, Reservation::Reserved),
    ("WITH", Keyword::With, Reservation::Reserved),
    ("WITHOUT", Keyword::Without, Reservation::NonReserved),
    ("WRITE", Keyword::Write, Reservation::NonReserved),
    ("XOR", Keyword::Xor, Reservation::Reserved),
    ("YEAR", Keyword::Year, Reservation::Reserved),
    ("YIELD", Keyword::Yield, Reservation::Reserved),
    ("ZONE", Keyword::Zone, Reservation::NonReserved),
    ("ZONED", Keyword::Zoned, Reservation::Reserved),
    (
        "ZONED_DATETIME",
        Keyword::ZonedDatetime,
        Reservation::Reserved,
    ),
    ("ZONED_TIME", Keyword::ZonedTime, Reservation::Reserved),
];

impl Keyword {
    /// The keyword's text, upper case.
    pub(crate) fn text(self) -> &'static str {
        match self {
            Keyword::Abs => "ABS",
            Keyword::Acos => "ACOS",
            Keyword::All => "ALL",
            Keyword::AllDifferent => "ALL_DIFFERENT",
            Keyword::And => "AND",
            Keyword::Any => "ANY",
            Keyword::Array => "ARRAY",
            Keyword::As => "AS",
            Keyword::Asc => "ASC",
            Keyword::Ascending => "ASCENDING",
            Keyword::Asin => "ASIN",
            Keyword::At => "AT",
            Keyword::Atan => "ATAN",
            Keyword::Avg => "AVG",
            Keyword::Big => "BIG",
            Keyword::Bigint => "BIGINT",
            Keyword::Binary => "BINARY",
            Keyword::Bool => "BOOL",
            Keyword::Boolean => "BOOLEAN",
            Keyword::Both => "BOTH",
            Keyword::Btrim => "BTRIM",
            Keyword::By => "BY",
            Keyword::ByteLength => "BYTE_LENGTH",
            Keyword::Bytes => "BYTES",
            Keyword::Call => "CALL",
            Keyword::Cardinality => "CARDINALITY",
            Keyword::Case => "CASE",
            Keyword::Cast => "CAST",
            Keyword::Ceil => "CEIL",
            Keyword::Ceiling => "CEILING",
            Keyword::Char => "CHAR",
            Keyword::CharLength => "CHAR_LENGTH",
            Keyword::CharacterLength => "CHARACTER_LENGTH",
            Keyword::Characteristics => "CHARACTERISTICS",
            Keyword::Close => "CLOSE",
            Keyword::Coalesce => "COALESCE",
            Keyword::CollectList => "COLLECT_LIST",
            Keyword::Commit => "COMMIT",
            Keyword::Copy => "COPY",
            Keyword::Cos => "COS",
            Keyword::Cosh => "COSH",
            Keyword::Cot => "COT",
            Keyword::Count => "COUNT",
            Keyword::Create => "CREATE",
            Keyword::CurrentDate => "CURRENT_DATE",
            Keyword::CurrentGraph => "CURRENT_GRAPH",
            Keyword::CurrentPropertyGraph => "CURRENT_PROPERTY_GRAPH",
            Keyword::CurrentSchema => "CURRENT_SCHEMA",
            Keyword::CurrentTime => "CURRENT_TIME",
            Keyword::CurrentTimestamp => "CURRENT_TIMESTAMP",
            Keyword::Date => "DATE",
            Keyword::Datetime => "DATETIME",
            Keyword::Day => "DAY",
            Keyword::Dec => "DEC",
            Keyword::Decimal => "DECIMAL",
            Keyword::Degrees => "DEGREES",
            Keyword::Delete => "DELETE",
            Keyword::Desc => "DESC",
            Keyword::Descending => "DESCENDING",
            Keyword::Detach => "DETACH",
            Keyword::Distinct => "DISTINCT",
            Keyword::Double => "DOUBLE",
            Keyword::Drop => "DROP",
            Keyword::Duration => "DURATION",
            Keyword::DurationBetween => "DURATION_BETWEEN",
            Keyword::ElementId => "ELEMENT_ID",
            Keyword::Else => "ELSE",
            Keyword::End => "END",
            Keyword::Except => "EXCEPT",
            Keyword::Exists => "EXISTS",
            Keyword::Exp => "EXP",
            Keyword::Filter => "FILTER",
            Keyword::Finish => "FINISH",
            Keyword::Float => "FLOAT",
            Keyword::Float16 => "FLOAT16",
            Keyword::Float32 => "FLOAT32",
            Keyword::Float64 => "FLOAT64",
            Keyword::Float128 => "FLOAT128",
            Keyword::Float256 => "FLOAT256",
            Keyword::Floor => "FLOOR",
            Keyword::For => "FOR",
            Keyword::From => "FROM",
            Keyword::Group => "GROUP",
            Keyword::Having => "HAVING",
            Keyword::HomeGraph => "HOME_GRAPH",
            Keyword::HomePropertyGraph => "HOME_PROPERTY_GRAPH",
            Keyword::HomeSchema => "HOME_SCHEMA",
            Keyword::Hour => "HOUR",
            Keyword::If => "IF",
            Keyword::In => "IN",
            Keyword::Insert => "INSERT",
            Keyword::Int => "INT",
            Keyword::Integer => "INTEGER",
            Keyword::Int8 => "INT8",
            Keyword::Integer8 => "INTEGER8",
            Keyword::Int16 => "INT16",
            Keyword::Integer16 => "INTEGER16",
            Keyword::Int32 => "INT32",
            Keyword::Integer32 => "INTEGER32",
            Keyword::Int64 => "INT64",
            Keyword::Integer64 => "INTEGER64",
            Keyword::Int128 => "INT128",
            Keyword::Integer128 => "INTEGER128",
            Keyword::Int256 => "INT256",
            Keyword::Integer256 => "INTEGER256",
            Keyword::Intersect => "INTERSECT",
            Keyword::Interval => "INTERVAL",
            Keyword::Is => "IS",
            Keyword::Leading => "LEADING",
            Keyword::Left => "LEFT",
            Keyword::Let => "LET",
            Keyword::Like => "LIKE",
            Keyword::Limit => "LIMIT",
            Keyword::List => "LIST",
            Keyword::Ln => "LN",
            Keyword::Local => "LOCAL",
            Keyword::LocalDatetime => "LOCAL_DATETIME",
            Keyword::LocalTime => "LOCAL_TIME",
            Keyword::LocalTimestamp => "LOCAL_TIMESTAMP",
            Keyword::Log => "LOG",
            Keyword::Log10 => "LOG10",
            Keyword::Lower => "LOWER",
            Keyword::Ltrim => "LTRIM",
            Keyword::Match => "MATCH",
            Keyword::Max => "MAX",
            Keyword::Min => "MIN",
            Keyword::Minute => "MINUTE",
            Keyword::Mod => "MOD",
            Keyword::Month => "MONTH",
            Keyword::Next => "NEXT",
            Keyword::Nodetach => "NODETACH",
            Keyword::Normalize => "NORMALIZE",
            Keyword::Not => "NOT",
            Keyword::Nothing => "NOTHING",
            Keyword::Null => "NULL",
            Keyword::Nulls => "NULLS",
            Keyword::Nullif => "NULLIF",
            Keyword::OctetLength => "OCTET_LENGTH",
            Keyword::Of => "OF",
            Keyword::Offset => "OFFSET",
            Keyword::Optional => "OPTIONAL",
            Keyword::Or => "OR",
            Keyword::Order => "ORDER",
            Keyword::Otherwise => "OTHERWISE",
            Keyword::Parameter => "PARAMETER",
            Keyword::Parameters => "PARAMETERS",
            Keyword::Path => "PATH",
            Keyword::PathLength => "PATH_LENGTH",
            Keyword::Paths => "PATHS",
            Keyword::PercentileCont => "PERCENTILE_CONT",
            Keyword::PercentileDisc => "PERCENTILE_DISC",
            Keyword::Power => "POWER",
            Keyword::Precision => "PRECISION",
            Keyword::PropertyExists => "PROPERTY_EXISTS",
            Keyword::Radians => "RADIANS",
            Keyword::Real => "REAL",
            Keyword::Record => "RECORD",
            Keyword::Remove => "REMOVE",
            Keyword::Replace => "REPLACE",
            Keyword::Reset => "RESET",
            Keyword::Return => "RETURN",
            Keyword::Right => "RIGHT",
            Keyword::Rollback => "ROLLBACK",
            Keyword::Rtrim => "RTRIM",
            Keyword::Same => "SAME",
            Keyword::Schema => "SCHEMA",
            Keyword::Second => "SECOND",
            Keyword::Select => "SELECT",
            Keyword::Session => "SESSION",
            Keyword::SessionUser => "SESSION_USER",
            Keyword::Set => "SET",
            Keyword::Signed => "SIGNED",
            Keyword::Sin => "SIN",
            Keyword::Sinh => "SINH",
            Keyword::Size => "SIZE",
            Keyword::Skip => "SKIP",
            Keyword::Small => "SMALL",
            Keyword::Smallint => "SMALLINT",
            Keyword::Sqrt => "SQRT",
            Keyword::Start => "START",
            Keyword::StddevPop => "STDDEV_POP",
            Keyword::StddevSamp => "STDDEV_SAMP",
            Keyword::String => "STRING",
            Keyword::Sum => "SUM",
            Keyword::Tan => "TAN",
            Keyword::Tanh => "TANH",
            Keyword::Then => "THEN",
            Keyword::Time => "TIME",
            Keyword::Timestamp => "TIMESTAMP",
            Keyword::Trailing => "TRAILING",
            Keyword::Trim => "TRIM",
            Keyword::Typed => "TYPED",
            Keyword::Ubigint => "UBIGINT",
            Keyword::Uint => "UINT",
            Keyword::Uint8 => "UINT8",
            Keyword::Uint16 => "UINT16",
            Keyword::Uint32 => "UINT32",
            Keyword::Uint64 => "UINT64",
            Keyword::Uint128 => "UINT128",
            Keyword::Uint256 => "UINT256",
            Keyword::Union => "UNION",
            Keyword::Unsigned => "UNSIGNED",
            Keyword::Upper => "UPPER",
            Keyword::Use => "USE",
            Keyword::Usmallint => "USMALLINT",
            Keyword::Value => "VALUE",
            Keyword::Varbinary => "VARBINARY",
            Keyword::Varchar => "VARCHAR",
            Keyword::Variable => "VARIABLE",
            Keyword::When => "WHEN",
            Keyword::Where => "WHERE",
            Keyword::With => "WITH",
            Keyword::Xor => "XOR",
            Keyword::Year => "YEAR",
            Keyword::Yield => "YIELD",
            Keyword::Zoned => "ZONED",
            Keyword::ZonedDatetime => "ZONED_DATETIME",
            Keyword::ZonedTime => "ZONED_TIME",
            Keyword::Abstract => "ABSTRACT",
            Keyword::Aggregate => "AGGREGATE",
            Keyword::Aggregates => "AGGREGATES",
            Keyword::Alter => "ALTER",
            Keyword::Catalog => "CATALOG",
            Keyword::Clear => "CLEAR",
            Keyword::Clone => "CLONE",
            Keyword::Constraint => "CONSTRAINT",
            Keyword::CurrentRole => "CURRENT_ROLE",
            Keyword::CurrentUser => "CURRENT_USER",
            Keyword::Data => "DATA",
            Keyword::Directory => "DIRECTORY",
            Keyword::Dryrun => "DRYRUN",
            Keyword::Exact => "EXACT",
            Keyword::Existing => "EXISTING",
            Keyword::Function => "FUNCTION",
            Keyword::Gqlstatus => "GQLSTATUS",
            Keyword::Grant => "GRANT",
            Keyword::Instant => "INSTANT",
            Keyword::Infinity => "INFINITY",
            Keyword::Number => "NUMBER",
            Keyword::Numeric => "NUMERIC",
            Keyword::On => "ON",
            Keyword::Open => "OPEN",
            Keyword::Partition => "PARTITION",
            Keyword::Procedure => "PROCEDURE",
            Keyword::Product => "PRODUCT",
            Keyword::Project => "PROJECT",
            Keyword::Query => "QUERY",
            Keyword::Records => "RECORDS",
            Keyword::Reference => "REFERENCE",
            Keyword::Rename => "RENAME",
            Keyword::Revoke => "REVOKE",
            Keyword::Substring => "SUBSTRING",
            Keyword::SystemUser => "SYSTEM_USER",
            Keyword::Temporal => "TEMPORAL",
            Keyword::Unique => "UNIQUE",
            Keyword::Unit => "UNIT",
            Keyword::Values => "VALUES",
            Keyword::Acyclic => "ACYCLIC",
            Keyword::Binding => "BINDING",
            Keyword::Bindings => "BINDINGS",
            Keyword::Connecting => "CONNECTING",
            Keyword::Destination => "DESTINATION",
            Keyword::Different => "DIFFERENT",
            Keyword::Directed => "DIRECTED",
            Keyword::Edge => "EDGE",
            Keyword::Edges => "EDGES",
            Keyword::Element => "ELEMENT",
            Keyword::Elements => "ELEMENTS",
            Keyword::First => "FIRST",
            Keyword::Graph => "GRAPH",
            Keyword::Groups => "GROUPS",
            Keyword::Keep => "KEEP",
            Keyword::Label => "LABEL",
            Keyword::Labeled => "LABELED",
            Keyword::Labels => "LABELS",
            Keyword::Last => "LAST",
            Keyword::Nfc => "NFC",
            Keyword::Nfd => "NFD",
            Keyword::Nfkc => "NFKC",
            Keyword::Nfkd => "NFKD",
            Keyword::No => "NO",
            Keyword::Node => "NODE",
            Keyword::Normalized => "NORMALIZED",
            Keyword::Only => "ONLY",
            Keyword::Ordinality => "ORDINALITY",
            Keyword::Property => "PROPERTY",
            Keyword::Read => "READ",
            Keyword::Relationship => "RELATIONSHIP",
            Keyword::Relationships => "RELATIONSHIPS",
            Keyword::Repeatable => "REPEATABLE",
            Keyword::Shortest => "SHORTEST",
            Keyword::Simple => "SIMPLE",
            Keyword::Source => "SOURCE",
            Keyword::Table => "TABLE",
            Keyword::To => "TO",
            Keyword::Trail => "TRAIL",
            Keyword::Transaction => "TRANSACTION",
            Keyword::Type => "TYPE",
            Keyword::Undirected => "UNDIRECTED",
            Keyword::Vertex => "VERTEX",
            Keyword::Walk => "WALK",
            Keyword::Without => "WITHOUT",
            Keyword::Write => "WRITE",
            Keyword::Zone => "ZONE",
        }
    }

    /// Whether the keyword is a non-reserved word, which the grammar
    /// also accepts as a regular identifier (`nonReservedWords`).
    pub(crate) fn is_non_reserved(self) -> bool {
        matches!(
            self,
            Keyword::Acyclic
                | Keyword::Binding
                | Keyword::Bindings
                | Keyword::Connecting
                | Keyword::Destination
                | Keyword::Different
                | Keyword::Directed
                | Keyword::Edge
                | Keyword::Edges
                | Keyword::Element
                | Keyword::Elements
                | Keyword::First
                | Keyword::Graph
                | Keyword::Groups
                | Keyword::Keep
                | Keyword::Label
                | Keyword::Labeled
                | Keyword::Labels
                | Keyword::Last
                | Keyword::Nfc
                | Keyword::Nfd
                | Keyword::Nfkc
                | Keyword::Nfkd
                | Keyword::No
                | Keyword::Node
                | Keyword::Normalized
                | Keyword::Only
                | Keyword::Ordinality
                | Keyword::Property
                | Keyword::Read
                | Keyword::Relationship
                | Keyword::Relationships
                | Keyword::Repeatable
                | Keyword::Shortest
                | Keyword::Simple
                | Keyword::Source
                | Keyword::Table
                | Keyword::To
                | Keyword::Trail
                | Keyword::Transaction
                | Keyword::Type
                | Keyword::Undirected
                | Keyword::Vertex
                | Keyword::Walk
                | Keyword::Without
                | Keyword::Write
                | Keyword::Zone
        )
    }
}
