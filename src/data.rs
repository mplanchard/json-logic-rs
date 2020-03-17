//! Data functions and operators
//!

use serde_json::{Map, Number, Value};
use std::convert::{From, TryFrom};
use crate::error::Error;
use crate::{NULL, Evaluated};


#[derive(Debug)]
pub enum KeyType<'a> {
    String(&'a String),
    Number(&'a Number),
}
impl From<KeyType<'_>> for Value {
    fn from(key: KeyType) -> Self {
        Value::from(&key)
    }
}
impl From<&KeyType<'_>> for Value {
    fn from(key: &KeyType) -> Self {
        match *key {
            KeyType::String(key) => Self::String(key.clone()),
            KeyType::Number(idx) => Self::Number(idx.clone()),
        }
    }
}

#[derive(Debug)]
pub struct Missing<'a> {
    values: Vec<KeyType<'a>>,
}
impl<'a> Missing<'a> {
    pub fn from_value(value: &'a Value) -> Result<Option<Self>, Error> {
        match value {
            Value::Object(obj) => {
                if let Some(val) = obj.get("missing") {
                    match val {
                        Value::Array(vals) => {
                            let mut vals_iter = vals.iter();
                            let first = vals_iter.next();
                            let missing_vals = match first {
                                None => Ok(vals),
                                // If the first value is a String, check to
                                // be sure the rest are too
                                Some(Value::String(_)) => {
                                    vals_iter.fold(Ok(()), |acc, each| {
                                        match each {
                                            Value::String(_) => acc,
                                            _ => Err(Error::InvalidOperation{
                                                key: "missing".into(),
                                                reason: format!("All 'missing' parameters must be of the same type. Expected String, got {:?}.", each)
                                            })
                                        }
                                    })?;
                                    Ok(vals)
                                }
                                // If the first value is a Number, check to
                                // be sure the rest are too
                                Some(Value::Number(_)) => {
                                    vals_iter.fold(Ok(()), |acc, each| {
                                        match each {
                                            Value::Number(_) => acc,
                                            _ => Err(Error::InvalidOperation{
                                                key: "missing".into(),
                                                reason: format!("All 'missing' parameter must be of the same type. Expected Number, got {:?}.", each)
                                            })
                                        }
                                    })?;
                                    Ok(vals)
                                }
                                _ => Err(Error::InvalidOperation {
                                    key: "missing".into(),
                                    reason: "'missing' parameters must be strings or numbers"
                                        .into(),
                                }),
                            }?;
                            let key_vals = missing_vals.iter().map(|val| {
                                match val {
                                    Value::String(key) => Ok(KeyType::String(key)),
                                    Value::Number(idx) => Ok(KeyType::Number(idx)),
                                    _ => Err(Error::UnexpectedError("Some keys were not strings or numbers even after validation".into()))
                                }
                            }).collect::<Result<Vec<KeyType>, Error>>()?;
                            Ok(Some(Missing { values: key_vals }))
                        }
                        _ => Err(Error::InvalidOperation {
                            key: "missing".into(),
                            reason: "Parameters to 'missing' must be an array.".into(),
                        }),
                    }
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    pub fn evaluate(&self, data: &Value) -> Result<Value, Error> {
        let vals = self
            .values
            .iter()
            .map(|v| get_key(data, v))
            .collect::<Result<Vec<Option<&Value>>, Error>>()?
            .iter()
            .zip(self.values.iter())
            .filter(|(val, _key)| val.is_none())
            .map(|(_val, key)| Value::from(key))
            .collect::<Vec<Value>>();
        Ok(Value::Array(vals))
    }
}
impl From<Missing<'_>> for Value {
    fn from(missing: Missing) -> Self {
        let mut map = Map::with_capacity(1);
        map.insert(
            "missing".into(),
            missing
                .values
                .into_iter()
                .map(|v| match v {
                    KeyType::String(key) => Value::String(key.clone()),
                    KeyType::Number(idx) => Value::Number(idx.clone()),
                })
                .collect(),
        );
        Self::Object(map)
    }
}


fn get_key<'a>(data: &'a Value, key: &KeyType) -> Result<Option<&'a Value>, Error> {
    match key {
        KeyType::String(key) => {
            match data {
                Value::Object(_) => key.split(".").fold(Ok(Some(data)), |acc, i| match acc? {
                    // If a previous key was not found, just send the None on through
                    None => Ok(None),
                    // If the current value is an object, try to get the value
                    Some(Value::Object(map)) => Ok(map.get(i)),
                    // If the current value is an array, we need an integer
                    // index. If integer conversion fails, return an error.
                    Some(Value::Array(arr)) => {
                        i.parse::<usize>()
                            .map(|i| arr.get(i))
                            .map_err(|_| Error::InvalidVariable {
                                value: Value::String(String::from(*key)),
                                reason: "Cannot access array data with non-integer key".into(),
                            })
                    }
                    _ => Ok(None),
                }),
                // We can only get string values off of objects. Anything else is an error.
                _ => Err(Error::InvalidData {
                    value: data.clone(),
                    reason: format!("Cannot get string key '{:?}' from non-object data", key),
                }),
            }
        }
        KeyType::Number(idx) => {
            match data {
                Value::Array(val) => {
                    idx
                        // Option<u64>
                        .as_u64()
                        // Result<u64, Error>
                        .ok_or(Error::InvalidVariable {
                            value: Value::Number((*idx).clone()),
                            reason: format!("Could not convert value to u64: {:?}", idx),
                        })
                        // Result<usize, Error>>
                        .and_then(|i| {
                            usize::try_from(i).map_err(|e| Error::InvalidVariable {
                                value: Value::Number((*idx).clone()),
                                reason: format!(
                                    "Could not convert value to a system-sized integer: {:?}",
                                    e
                                ),
                            })
                        })
                        // Result<Option<Value>, Error>
                        .map(|idx| val.get(idx))
                }
                _ => Err(Error::InvalidVariable {
                    value: Value::Number((*idx).clone()),
                    reason: "Cannot access non-array data with an index variable".into(),
                }),
            }
        }
    }
}


#[derive(Debug)]
pub struct Variable<'a> {
    value: &'a Value,
}
impl<'a> Variable<'a> {
    pub fn from_value(value: &'a Value) -> Result<Option<Self>, Error> {
        match value {
            Value::Object(map) => {
                if map.len() != 1 {
                    return Ok(None);
                };
                match map.get("var") {
                    Some(var) => match var {
                        Value::String(_) => Ok(Some(Variable { value: var })),
                        Value::Number(_) => Ok(Some(Variable { value: var })),
                        Value::Array(arr) => match arr.len() {
                            0..=2 => Ok(Some(Variable { value: var })),
                            _ => Err(Error::InvalidVariable {
                                value: value.clone(),
                                reason: "Array variables must be of len 0..2 inclusive".into(),
                            }),
                        },
                        _ => Err(Error::InvalidVariable {
                            value: value.clone(),
                            reason: "Variables must be strings, integers, or arrays".into(),
                        }),
                    },
                    None => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    pub fn evaluate(&self, data: &'a Value) -> Result<&'a Value, Error> {
        // if self.name == "" { return data };
        match self.value {
            Value::Null => Ok(data),
            Value::String(var_name) => self.interpolate_string_var(data, var_name),
            Value::Number(idx) => self.interpolate_numeric_var(data, idx),
            Value::Array(var) => self.interpolate_array_var(data, var),
            _ => Err(Error::InvalidVariable{
                value: self.value.clone(),
                reason: "Unsupported variable type. Variables must be strings, integers, arrays, or null.".into()
            })
        }
    }

    fn get_default(&self) -> &'a Value {
        match self.value {
            Value::Array(val) => val.get(1).unwrap_or(&NULL),
            _ => &NULL,
        }
    }

    fn interpolate_array_var(
        &self,
        data: &'a Value,
        var: &'a Vec<Value>,
    ) -> Result<&'a Value, Error> {
        let len = var.len();
        match len {
            0 => Ok(data),
            1 | 2 => match &var[0] {
                Value::String(var_name) => self.interpolate_string_var(data, &var_name),
                Value::Number(var_idx) => self.interpolate_numeric_var(data, &var_idx),
                _ => Err(Error::InvalidVariable {
                    value: Value::Array(var.clone()),
                    reason: "Variables must be strings or integers".into(),
                }),
            },
            _ => Err(Error::InvalidVariable {
                value: Value::Array(var.clone()),
                reason: format!("Array variables must be of len 1 or 2, not {}", len),
            }),
        }
    }

    fn interpolate_numeric_var(
        &self,
        data: &'a Value,
        idx: &'a Number,
    ) -> Result<&'a Value, Error> {
        let default = self.get_default();
        match data {
            Value::Array(val) => {
                idx
                    // Option<u64>
                    .as_u64()
                    // Option<Result<usize, Error>>
                    .map(|i| {
                        usize::try_from(i).map_err(|e| Error::InvalidVariable {
                            value: Value::Number(idx.clone()),
                            reason: format!(
                                "Could not convert value to a system-sized integer: {:?}",
                                e
                            ),
                        })
                    })
                    // Option<Result<Value, Error>>
                    .map(|res| res.map(|i| val.get(i).unwrap_or(default)))
                    // Result<Value, Error>
                    .unwrap_or(Ok(default))
            }
            _ => Err(Error::InvalidVariable {
                value: Value::Number(idx.clone()),
                reason: "Cannot access non-array data with an index variable".into(),
            }),
        }
    }

    fn interpolate_string_var(
        &self,
        data: &'a Value,
        var_name: &'a String,
    ) -> Result<&'a Value, Error> {
        if var_name == "" {
            return Ok(data);
        };
        let key = KeyType::String(var_name);
        get_key(data, &key).map(|v| v.unwrap_or(self.get_default()))
    }
}
impl<'a> From<Variable<'a>> for Value {
    fn from(var: Variable) -> Self {
        let mut map = Map::with_capacity(1);
        map.insert("var".into(), var.value.clone());
        Value::Object(map)
    }
}
