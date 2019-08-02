use crate::ApiError;
use hyper::Request;

/// Provides handy functions for parsing the query parameters of a URL.
pub struct UrlQuery<'a>(url::form_urlencoded::Parse<'a>);

impl<'a> UrlQuery<'a> {
    /// Instantiate from an existing `Request`.
    ///
    /// Returns `Err` if `req` does not contain any query parameters.
    pub fn from_request<T>(req: &'a Request<T>) -> Result<Self, ApiError> {
        let query_str = req.uri().query().ok_or_else(|| {
            ApiError::InvalidQueryParams(
                "URL query must be valid and contain at least one
                                         key."
                    .to_string(),
            )
        })?;

        Ok(UrlQuery(url::form_urlencoded::parse(query_str.as_bytes())))
    }

    /// Returns the first `(key, value)` pair found where the `key` is in `keys`.
    ///
    /// If no match is found, an `InvalidQueryParams` error is returned.
    pub fn first_of(mut self, keys: &[&str]) -> Result<(String, String), ApiError> {
        self.0
            .find(|(key, _value)| keys.contains(&&**key))
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .ok_or_else(|| {
                ApiError::InvalidQueryParams(format!(
                    "URL query must contain at least one of the following keys: {:?}",
                    keys
                ))
            })
    }

    pub fn only_one(self, key: &str) -> Result<String, ApiError> {
        let queries: Vec<_> = self
            .0
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();

        if queries.len() == 1 {
            let (first_key, first_value) = &queries[0]; // Must have 0 index if len is 1.
            if first_key == key {
                Ok(first_value.to_string())
            } else {
                Err(ApiError::InvalidQueryParams(format!(
                    "Only the {} query parameter is supported",
                    key
                )))
            }
        } else {
            Err(ApiError::InvalidQueryParams(format!(
                "Only one query parameter is allowed, {} supplied",
                queries.len()
            )))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn query_params_first_of() {
        let url = url::Url::parse("http://lighthouse.io/cats?a=42&b=12&c=100").unwrap();
        let get_query = || UrlQuery(url.query_pairs());

        assert_eq!(
            get_query().first_of(&["a"]),
            Ok(("a".to_string(), "42".to_string()))
        );
        assert_eq!(
            get_query().first_of(&["a", "b", "c"]),
            Ok(("a".to_string(), "42".to_string()))
        );
        assert_eq!(
            get_query().first_of(&["a", "a", "a"]),
            Ok(("a".to_string(), "42".to_string()))
        );
        assert_eq!(
            get_query().first_of(&["a", "b", "c"]),
            Ok(("a".to_string(), "42".to_string()))
        );
        assert_eq!(
            get_query().first_of(&["b", "c"]),
            Ok(("b".to_string(), "12".to_string()))
        );
        assert_eq!(
            get_query().first_of(&["c"]),
            Ok(("c".to_string(), "100".to_string()))
        );
        assert!(get_query().first_of(&["nothing"]).is_err());
    }
}
