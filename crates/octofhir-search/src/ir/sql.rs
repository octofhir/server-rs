/// Small PostgreSQL SQL expression AST used by IR renderers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlExpr {
    And(Vec<SqlExpr>),
    Or(Vec<SqlExpr>),
    Not(Box<SqlExpr>),
    Exists(Box<SelectStmt>),
    Compare {
        lhs: SqlTerm,
        op: SqlOp,
        rhs: SqlTerm,
    },
    IsNull(SqlTerm),
    IsNotNull(SqlTerm),
    RangeOp {
        lhs: SqlTerm,
        op: RangeOp,
        rhs: SqlTerm,
    },
    Bool(bool),
    Raw(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectStmt {
    pub projection: Vec<SqlTerm>,
    pub from: SqlFrom,
    pub where_clause: Option<SqlExpr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlFrom {
    pub table: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlTerm {
    Ident(String),
    Param(usize),
    ParamCast {
        index: usize,
        cast: &'static str,
    },
    Expr(Box<SqlExpr>),
    TimestampRange {
        lo: Box<SqlTerm>,
        hi: Box<SqlTerm>,
        bounds: &'static str,
    },
    Bool(bool),
    Integer(i64),
    Null,
    Raw(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlOp {
    Eq,
    Ne,
    Like,
    ILike,
    JsonbContains,
    Gt,
    Lt,
    Ge,
    Le,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeOp {
    ContainsBy,
    Overlaps,
    StrictlyAfter,
    StrictlyBefore,
}
