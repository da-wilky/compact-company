use axum::http::Method;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// What an API token may do; tokens never issue, send or book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
pub enum Scope {
    /// Customers, projects, activities, todos and time: read.
    #[serde(rename = "work:read")]
    WorkRead,
    #[serde(rename = "work:write")]
    WorkWrite,
    /// Documents and their drafts, snippets and layouts: read.
    #[serde(rename = "documents:read")]
    DocumentsRead,
    /// Drafts and snippets: write; never issuing, sending or recording payments.
    #[serde(rename = "documents:write")]
    DocumentsWrite,
    /// The books, tax figures, exports and deadlines: read only.
    #[serde(rename = "books:read")]
    BooksRead,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkRead => "work:read",
            Self::WorkWrite => "work:write",
            Self::DocumentsRead => "documents:read",
            Self::DocumentsWrite => "documents:write",
            Self::BooksRead => "books:read",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::from(value)).ok()
    }
}

/// The scope a token needs for a request, or none for what a token may never do.
pub fn required(method: &Method, path: &str) -> Option<Scope> {
    let path = path.strip_prefix("/api").unwrap_or(path);
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let read = method.is_safe();
    let pick = |read_scope, write_scope| Some(if read { read_scope } else { write_scope });
    match segments[0] {
        "customers" | "projects" | "activities" | "statuses" | "todos" | "assignees" | "time"
        | "timers" | "currencies" => pick(Scope::WorkRead, Scope::WorkWrite),
        "documents"
            if !read && matches!(segments.get(2), Some(&("issue" | "send" | "payments"))) =>
        {
            None
        }
        "documents" | "snippets" | "unbilled" => pick(Scope::DocumentsRead, Scope::DocumentsWrite),
        "layouts" | "layout-choices" | "packages" if read => Some(Scope::DocumentsRead),
        "books" | "accounts" | "postings" | "receipts" | "suppliers" | "bank-accounts"
        | "bank-transactions" | "open-items" | "tax" | "exports" | "deadlines" | "years"
            if read =>
        {
            Some(Scope::BooksRead)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_never_issue_send_book_or_administer() {
        let need = |method: Method, path: &str| required(&method, path);
        assert_eq!(need(Method::GET, "/api/time"), Some(Scope::WorkRead));
        assert_eq!(need(Method::POST, "/api/time"), Some(Scope::WorkWrite));
        assert_eq!(
            need(Method::PUT, "/api/documents/3"),
            Some(Scope::DocumentsWrite)
        );
        assert_eq!(need(Method::POST, "/api/documents/3/issue"), None);
        assert_eq!(need(Method::POST, "/api/documents/3/send"), None);
        assert_eq!(need(Method::POST, "/api/documents/3/payments"), None);
        assert_eq!(need(Method::GET, "/api/postings"), Some(Scope::BooksRead));
        assert_eq!(need(Method::POST, "/api/postings"), None);
        assert_eq!(need(Method::POST, "/api/receipts/1/book"), None);
        assert_eq!(need(Method::POST, "/api/layouts"), None);
        for path in ["/api/users", "/api/tokens", "/api/business", "/api/audit"] {
            assert_eq!(need(Method::GET, path), None, "{path}");
        }
        assert_eq!(Scope::parse("books:read"), Some(Scope::BooksRead));
        assert_eq!(Scope::parse("books:write"), None);
    }
}
