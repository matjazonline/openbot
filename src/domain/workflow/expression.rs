//! Typed comparison semantics shared by pure workflow bindings and rules.
use super::{Comparison, ContextError};
use bigdecimal::BigDecimal;
use serde_json::Value;
use std::{cmp::Ordering, str::FromStr};

fn decimal(value: &serde_json::Number) -> BigDecimal {
    BigDecimal::from_str(&value.to_string()).expect("JSON numbers are finite decimals")
}

/// JSON equality compares numbers by exact decimal value (1 equals 1.0),
/// recursively. It never converts an integer through f64.
pub(super) fn equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => decimal(a) == decimal(b),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| equal(x, y))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|(k, x)| b.get(k).is_some_and(|y| equal(x, y)))
        }
        _ => left == right,
    }
}

pub(super) fn compare(op: Comparison, left: &Value, right: &Value) -> Result<bool, ContextError> {
    match op {
        Comparison::Eq => Ok(equal(left, right)),
        Comparison::Ne => Ok(!equal(left, right)),
        _ => {
            let order = match (left, right) {
                (Value::Number(a), Value::Number(b)) => decimal(a).cmp(&decimal(b)),
                (Value::String(a), Value::String(b)) => a.cmp(b),
                _ => {
                    return Err(ContextError::Operand(
                        "ordering expects two numbers or two strings",
                    ));
                }
            };
            Ok(match op {
                Comparison::Lt => order == Ordering::Less,
                Comparison::Le => order != Ordering::Greater,
                Comparison::Gt => order == Ordering::Greater,
                Comparison::Ge => order != Ordering::Less,
                Comparison::Eq | Comparison::Ne => unreachable!(),
            })
        }
    }
}
