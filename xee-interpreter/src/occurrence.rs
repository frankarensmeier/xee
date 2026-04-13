use crate::error;

pub fn one<'a, T>(mut iter: impl Iterator<Item = error::Result<T>> + 'a) -> error::Result<T> {
    if let Some(one) = iter.next() {
        if iter.next().is_none() {
            Ok(one?)
        } else {
            Err(error::Error::type_error(
                "expected exactly one item, got more",
            ))
        }
    } else {
        Err(error::Error::type_error(
            "expected exactly one item, got empty sequence",
        ))
    }
}

pub fn option<'a, T>(
    mut iter: impl Iterator<Item = error::Result<T>> + 'a,
) -> error::Result<Option<T>> {
    if let Some(one) = iter.next() {
        if iter.next().is_none() {
            Ok(Some(one?))
        } else {
            Err(error::Error::type_error(
                "expected zero or one item, got more",
            ))
        }
    } else {
        Ok(None)
    }
}
