use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::hash::Hash;

pub struct TableState {
  integer: RwLock<BTreeMap<i64, TableValue>>,
  other: DashMap<TableKey, TableValue>,
}

impl TableState {
  pub fn new() -> Self {
    Self {
      integer: RwLock::new(BTreeMap::new()),
      other: DashMap::new(),
    }
  }

  pub fn set(&self, key: TableKey, value: TableValue) -> TableValue {
    let result = if let TableValue::Integer(x) = key.0 {
      self.integer.write().insert(x, value)
    } else {
      self.other.insert(key, value)
    };
    result.unwrap_or(TableValue::Nil)
  }

  /// Corresponds to Lua's table behaviour - `#table` may return any "border"
  /// (and in this case, the last one)
  pub fn len(&self) -> i64 {
    match self.integer.read().iter().nth_back(0) {
      Some((&x, _)) if x > 0 => x,
      Some(_) | None => 0,
    }
  }
}

pub enum TableValue {
  Nil,
  Boolean(bool),
  Integer(i64),
  Number(f64),
  String(Vec<u8>),
  Table(TableState),
}

pub struct TableKey(TableValue);

impl TableKey {
  pub fn from_value(v: TableValue) -> Result<Self, InvalidTableKey> {
    use TableValue::*;
    match v {
      Nil | Table(_) => Err(InvalidTableKey(())),
      Number(x) if x.is_nan() => Err(InvalidTableKey(())),
      Number(x) => {
        let i = x as i64;
        if i as f64 == x {
          Ok(Self(Integer(i)))
        } else {
          Ok(Self(Number(x)))
        }
      }
      _ => Ok(Self(v)),
    }
  }
}

impl Hash for TableKey {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    use TableValue::*;
    match &self.0 {
      Nil | Table(_) => unreachable!(),
      Boolean(x) => (1u8, x).hash(state),
      Integer(x) => (2u8, x).hash(state),
      Number(x) => (3u8, canonical_float_bytes(*x)).hash(state),
      String(x) => (2u8, x).hash(state),
    }
  }
}

impl PartialEq for TableKey {
  fn eq(&self, other: &Self) -> bool {
    use TableValue::*;
    match (&self.0, &other.0) {
      (Nil, _) | (Table(_), _) | (_, Nil) | (_, Table(_)) => unreachable!(),
      (Boolean(x), Boolean(y)) => x == y,
      (Boolean(_), _) => false,
      (Integer(x), Integer(y)) => x == y,
      (Integer(x), Number(y)) => *x as f64 == *y,
      (Integer(_), _) => false,
      (Number(x), Number(y)) => x == y,
      (Number(x), Integer(y)) => *x == *y as f64,
      (Number(_), _) => false,
      (String(x), String(y)) => x == y,
      (String(_), _) => false,
    }
  }
}

impl Eq for TableKey {}

/// copied from https://github.com/kyren/luster
fn canonical_float_bytes(f: f64) -> u64 {
  assert!(!f.is_nan());
  unsafe {
    if f == 0.0 {
      std::mem::transmute(0.0f64)
    } else {
      std::mem::transmute(f)
    }
  }
}

pub struct InvalidTableKey(());
