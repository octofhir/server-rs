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
    RangeOp {
        lhs: SqlTerm,
        op: RangeOp,
        rhs: SqlTerm,
    },
    Raw(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectStmt {
    pub sql: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlTerm {
    Ident(String),
    Param(usize),
    TimestampRange {
        lo: Box<SqlTerm>,
        hi: Box<SqlTerm>,
        bounds: &'static str,
    },
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlOp {
    Eq,
    Ne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeOp {
    ContainsBy,
    Overlaps,
    StrictlyAfter,
    StrictlyBefore,
}
