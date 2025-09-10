use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum QueryNode {
    Term(String),
    Field(String, String),
    And(Box<QueryNode>, Box<QueryNode>),
    Or(Box<QueryNode>, Box<QueryNode>),
    Not(Box<QueryNode>),
    Group(Box<QueryNode>),
    Range(String, String, String), // field, start, end
    Wildcard(String),
    Fuzzy(String, u8), // term, distance
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedQuery {
    pub ast: QueryNode,
}

impl ParsedQuery {
    pub fn new(ast: QueryNode) -> Self {
        Self { ast }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdvancedQueryError {
    QueryParseError(String),
    InvalidFieldError(String),
    InvalidRangeError(String),
    InvalidBooleanOperatorError(String),
    UnmatchedParenthesesError,
    QueryTooComplexError,
    QueryTimeoutError,
    InternalError(String),
}

impl fmt::Display for AdvancedQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdvancedQueryError::QueryParseError(msg) => write!(f, "Query parse error: {}", msg),
            AdvancedQueryError::InvalidFieldError(field) => write!(f, "Invalid field: {}", field),
            AdvancedQueryError::InvalidRangeError(range) => write!(f, "Invalid range: {}", range),
            AdvancedQueryError::InvalidBooleanOperatorError(op) => write!(f, "Invalid boolean operator: {}", op),
            AdvancedQueryError::UnmatchedParenthesesError => write!(f, "Unmatched parentheses"),
            AdvancedQueryError::QueryTooComplexError => write!(f, "Query too complex"),
            AdvancedQueryError::QueryTimeoutError => write!(f, "Query timeout"),
            AdvancedQueryError::InternalError(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for AdvancedQueryError {}

pub struct AdvancedQueryParser;

impl AdvancedQueryParser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse(&self, input: &str) -> Result<ParsedQuery, AdvancedQueryError> {
        // For simplicity, we'll just parse a basic field:value query
        // A real implementation would be much more complex
        if input.is_empty() {
            return Ok(ParsedQuery::new(
                QueryNode::Term("".to_string()),
            ));
        }

        // Try to parse as field:value
        if let Some(pos) = input.find(':') {
            let field = &input[..pos];
            let value = &input[pos + 1..];
            
            // Handle quoted values
            let value = if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                &value[1..value.len() - 1]
            } else {
                value
            };
            
            return Ok(ParsedQuery::new(
                QueryNode::Field(field.to_string(), value.to_string()),
            ));
        }

        // Handle simple term
        Ok(ParsedQuery::new(
            QueryNode::Term(input.to_string()),
        ))
    }
}