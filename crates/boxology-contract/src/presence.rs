/// The contract side on which a value is being decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecodeRole {
    ProviderInput,
    ConsumerOutput,
}

/// Presence and nullability for a top-level slot or object field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field<T> {
    Missing,
    Null,
    Value(T),
}

impl<T> Field<T> {
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn is_value(&self) -> bool {
        matches!(self, Self::Value(_))
    }

    /// Borrows the value, collapsing both `Missing` and `Null` to `None`.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Missing | Self::Null => None,
        }
    }

    /// Takes the value, collapsing both `Missing` and `Null` to `None`.
    pub fn into_value(self) -> Option<T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Missing | Self::Null => None,
        }
    }

    pub fn as_ref(&self) -> Field<&T> {
        match self {
            Self::Missing => Field::Missing,
            Self::Null => Field::Null,
            Self::Value(value) => Field::Value(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Field;

    #[test]
    fn field_queries_and_projections_preserve_all_states() {
        let missing = Field::<u8>::Missing;
        let null = Field::<u8>::Null;
        let value = Field::Value(7);
        assert!(missing.is_missing() && null.is_null() && value.is_value());
        assert_eq!((missing.value(), missing.as_ref()), (None, Field::Missing));
        assert_eq!((null.as_ref(), null.into_value()), (Field::Null, None));
        assert_eq!(value.value(), Some(&7));
        assert_eq!(value.as_ref(), Field::Value(&7));
        assert_eq!(value.into_value(), Some(7));
    }
}
