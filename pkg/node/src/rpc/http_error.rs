use super::routes;
use crate::errors;
use element::Element;
use rpc::{code::ErrorCode, error::HTTPError};
use serde::Serialize;

impl From<routes::error::Error> for HTTPError {
    fn from(err: routes::error::Error) -> Self {
        match err {
            routes::error::Error::InvalidElement(..) => HTTPError::new(
                ErrorCode::BadRequest,
                "invalid-element",
                Some(err.into()),
                None::<()>,
            ),
            routes::error::Error::OutOfSync => HTTPError::new(
                ErrorCode::Unavailable,
                "out-of-sync",
                Some(err.into()),
                None::<()>,
            ),
            routes::error::Error::StatisticsNotReady => HTTPError::new(
                ErrorCode::Unavailable,
                "statistics-not-ready",
                Some(err.into()),
                None::<()>,
            ),
            routes::error::Error::InvalidListQuery(err) => HTTPError::new(
                ErrorCode::BadRequest,
                "invalid-list-query",
                Some(err.into()),
                None::<()>,
            ),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ElementData {
    pub element: Element,
}

#[derive(Debug, Serialize)]
pub struct HashData {
    pub hash: Element,
}

#[derive(Debug, Serialize)]
pub struct ElementStringData {
    pub element: String,
}

impl From<errors::Error> for HTTPError {
    fn from(err: errors::Error) -> Self {
        match err {
            errors::Error::Rpc(rpc_error) => rpc_error.into(),
            _ => HTTPError::new(
                ErrorCode::Internal,
                "internal",
                Some(err.into()),
                None::<()>,
            ),
        }
    }
}
