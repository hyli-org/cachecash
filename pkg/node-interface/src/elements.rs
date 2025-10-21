use element::Element;
use serde::{Deserialize, Serialize};

/// Query for list elemenets
#[derive(Debug, Serialize, Deserialize)]
pub struct ListElementsQuery {
    /// String comma seperated list of elements to lookup
    pub elements: String,
    /// When true, include elements that have been spent (seen historically)
    #[serde(default)]
    pub include_spent: bool,
}

/// Response from the elements endpoint
pub type ElementsResponse = Vec<ElementsResponseSingle>;

/// Response item from the elements endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementsResponseSingle {
    /// The element being returned
    pub element: Element,
    /// Block height that the element was included in
    pub height: u64,
    /// Root hash of the block the element was included in
    pub root_hash: Element,
    /// Txn hash
    pub txn_hash: Element,
    /// Whether the element has been spent
    pub spent: bool,
}
