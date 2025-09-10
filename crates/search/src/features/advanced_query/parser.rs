use nom::{
    branch::alt,
    bytes::complete::{tag, take_until, take_while, take_while1},
    character::complete::{alpha1, alphanumeric1, char, multispace0, multispace1},
    combinator::{map, opt, recognize},
    error::{Error, ErrorKind},
    multi::{many0, separated_list0},
    sequence::{delimited, pair, preceded, separated_pair, terminated, tuple},
    IResult,
};
use std::time::Instant;
use tracing::{debug, trace};

use crate::features::advanced_query::{
    dto::{ParsedQueryInfo, FieldQuery, BooleanOperator, QueryOperator},
    error::AdvancedQueryError,
};

#[derive(Debug, Clone)]
pub struct AdvancedQueryParser;

#[derive(Debug, Clone)]
pub struct ParsedQuery {
    pub original_query: String,
    pub field_queries: Vec<FieldQuery>,
    pub boolean_operators: Vec<BooleanOperator>,
    pub has_wildcards: bool,
    pub has_fuzzy: bool,
    pub has_ranges: bool,
}

impl AdvancedQueryParser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse(&self, query: &str) -> Result<ParsedQuery, AdvancedQueryError> {
        debug!(query = %query, "Parsing advanced query");
        let start_time = Instant::now();

        // Check for query complexity (nesting depth, length, etc.)
        if query.len() > 1000 {
            return Err(AdvancedQueryError::QueryTooComplexError);
        }

        // Parse the query
        let parsed_result = self.parse_query_internal(query)
            .map_err(|e| AdvancedQueryError::QueryParseError(format!("Parse error at position {}: {:?}", e.input.len(), e.code)))?;

        let parsed_query = parsed_result.1;

        // Check for timeout
        let duration = start_time.elapsed();
        if duration.as_millis() > 10 {
            return Err(AdvancedQueryError::QueryTimeoutError);
        }

        debug!(duration_ms = duration.as_millis(), "Query parsed successfully");
        Ok(parsed_query)
    }

    fn parse_query_internal(&self, input: &str) -> IResult<&str, ParsedQuery> {
        // Parse field queries separated by boolean operators
        let (input, first_query) = self.parse_field_query(input)?;
        
        let mut field_queries = vec![first_query];
        let mut boolean_operators = Vec::new();
        let mut has_wildcards = first_query.value.contains('*') || first_query.value.contains('?');
        let mut has_fuzzy = first_query.value.contains('~');
        let mut has_ranges = matches!(first_query.operator, QueryOperator::InRange);

        let (input, remainder) = many0(|input| {
            let (input, _) = multispace0(input)?;
            let (input, op) = self.parse_boolean_operator(input)?;
            let (input, _) = multispace0(input)?;
            let (input, query) = self.parse_field_query(input)?;
            
            boolean_operators.push(op);
            field_queries.push(query);
            
            if query.value.contains('*') || query.value.contains('?') {
                has_wildcards = true;
            }
            
            if query.value.contains('~') {
                has_fuzzy = true;
            }
            
            if matches!(query.operator, QueryOperator::InRange) {
                has_ranges = true;
            }
            
            Ok((input, ()))
        })(input)?;

        Ok((remainder, ParsedQuery {
            original_query: input.to_string(),
            field_queries,
            boolean_operators,
            has_wildcards,
            has_fuzzy,
            has_ranges,
        }))
    }

    fn parse_field_query(&self, input: &str) -> IResult<&str, FieldQuery> {
        trace!(input = %input, "Parsing field query");
        
        // Try to parse field-specific query first
        let field_query_result = self.parse_field_specific_query(input);
        if let Ok(result) = field_query_result {
            return Ok(result);
        }

        // If field-specific parsing fails, treat as a general text query
        self.parse_general_query(input)
    }

    fn parse_field_specific_query(&self, input: &str) -> IResult<&str, FieldQuery> {
        trace!(input = %input, "Parsing field-specific query");
        
        let (input, (field, _, value)) = tuple((
            self.parse_identifier,
            char(':'),
            self.parse_quoted_or_unquoted_value,
        ))(input)?;

        // Determine operator based on value
        let (operator, clean_value) = if value.starts_with('[') && value.ends_with(']') {
            (QueryOperator::InRange, value)
        } else if value.contains('*') || value.contains('?') {
            (QueryOperator::Contains, value)
        } else if value.contains('~') {
            (QueryOperator::Contains, value)
        } else {
            (QueryOperator::Equals, value)
        };

        Ok((input, FieldQuery {
            field: field.to_string(),
            value: clean_value.to_string(),
            operator,
        }))
    }

    fn parse_general_query(&self, input: &str) -> IResult<&str, FieldQuery> {
        trace!(input = %input, "Parsing general query");
        
        let (input, value) = self.parse_quoted_or_unquoted_value(input)?;
        
        // For general queries, we'll search across multiple default fields
        Ok((input, FieldQuery {
            field: "default".to_string(),
            value: value.to_string(),
            operator: QueryOperator::Contains,
        }))
    }

    fn parse_identifier(&self, input: &str) -> IResult<&str, &str> {
        trace!(input = %input, "Parsing identifier");
        
        recognize(pair(
            alt((alpha1, tag("_"))),
            many0(alt((alphanumeric1, tag("_")))),
        ))(input)
    }

    fn parse_quoted_or_unquoted_value(&self, input: &str) -> IResult<&str, &str> {
        trace!(input = %input, "Parsing quoted or unquoted value");
        
        // Try quoted value first
        let quoted_result = delimited(
            char('"'),
            take_until("\""),
            char('"'),
        )(input);
        
        if let Ok(result) = quoted_result {
            return Ok(result);
        }

        // If quoted parsing fails, try unquoted value
        self.parse_unquoted_value(input)
    }

    fn parse_unquoted_value(&self, input: &str) -> IResult<&str, &str> {
        trace!(input = %input, "Parsing unquoted value");
        
        recognize(
            take_while1(|c: char| !c.is_whitespace() && c != ')' && c != '(')
        )(input)
    }

    fn parse_boolean_operator(&self, input: &str) -> IResult<&str, BooleanOperator> {
        trace!(input = %input, "Parsing boolean operator");
        
        let (input, op) = alt((
            map(tag("AND"), |_| BooleanOperator::And),
            map(tag("OR"), |_| BooleanOperator::Or),
            map(tag("NOT"), |_| BooleanOperator::Not),
            map(tag("&&"), |_| BooleanOperator::And),
            map(tag("||"), |_| BooleanOperator::Or),
            map(tag("!"), |_| BooleanOperator::Not),
        ))(input)?;

        Ok((input, op))
    }
}

impl ParsedQuery {
    pub fn to_parsed_query_info(&self) -> ParsedQueryInfo {
        ParsedQueryInfo {
            original_query: self.original_query.clone(),
            parsed_fields: self.field_queries.clone(),
            boolean_operators: self.boolean_operators.clone(),
            has_wildcards: self.has_wildcards,
            has_fuzzy: self.has_fuzzy,
            has_ranges: self.has_ranges,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_field_query() {
        let parser = AdvancedQueryParser::new();
        let result = parser.parse("name:test");
        assert!(result.is_ok());
        
        let parsed = result.unwrap();
        assert_eq!(parsed.field_queries.len(), 1);
        assert_eq!(parsed.field_queries[0].field, "name");
        assert_eq!(parsed.field_queries[0].value, "test");
    }

    #[test]
    fn test_parse_quoted_value() {
        let parser = AdvancedQueryParser::new();
        let result = parser.parse("name:\"test value\"");
        assert!(result.is_ok());
        
        let parsed = result.unwrap();
        assert_eq!(parsed.field_queries.len(), 1);
        assert_eq!(parsed.field_queries[0].field, "name");
        assert_eq!(parsed.field_queries[0].value, "test value");
    }

    #[test]
    fn test_parse_boolean_operators() {
        let parser = AdvancedQueryParser::new();
        let result = parser.parse("name:test AND version:1.0");
        assert!(result.is_ok());
        
        let parsed = result.unwrap();
        assert_eq!(parsed.field_queries.len(), 2);
        assert_eq!(parsed.boolean_operators.len(), 1);
        assert_eq!(parsed.boolean_operators[0], BooleanOperator::And);
    }

    #[test]
    fn test_parse_wildcard_query() {
        let parser = AdvancedQueryParser::new();
        let result = parser.parse("name:test*");
        assert!(result.is_ok());
        
        let parsed = result.unwrap();
        assert!(parsed.has_wildcards);
    }

    #[test]
    fn test_parse_range_query() {
        let parser = AdvancedQueryParser::new();
        let result = parser.parse("size:[1000 TO 5000]");
        assert!(result.is_ok());
        
        let parsed = result.unwrap();
        assert!(parsed.has_ranges);
    }
}