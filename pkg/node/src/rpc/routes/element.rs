use crate::{Error, Result};

use super::{State, error};
use actix_web::web;
use block_store::Block;
use element::Element;
use node_interface::{
    ElementData, ElementsResponse, ElementsResponseSingle, ListElementsQuery, RpcError,
};
use rpc::error::HttpResult;
use std::str::FromStr;

#[tracing::instrument(err, skip_all)]
pub async fn get_element(
    state: web::Data<State>,
    path: web::Path<(Element,)>,
) -> HttpResult<web::Json<ElementsResponseSingle>> {
    let (element,) = path.into_inner();
    Ok(web::Json(get_element_response(&state, element, false)?))
}

#[tracing::instrument(err, skip_all)]
pub async fn list_elements(
    state: web::Data<State>,
    query: web::Query<ListElementsQuery>,
) -> HttpResult<web::Json<ElementsResponse>> {
    if query.elements.is_empty() {
        return Ok(web::Json(vec![]));
    }

    let elements = query
        .elements
        .split(',')
        .map(|c| {
            Element::from_str(c)
                .map_err(|e| error::Error::InvalidElement(c.to_string(), e))
                .map_err(rpc::error::HTTPError::from)
        })
        .collect::<HttpResult<Vec<Element>>>()?;

    Ok(web::Json(
        elements
            .iter()
            .map(
                |element| match get_element_response(&state, *element, query.include_spent) {
                    Ok(response) => Ok(Some(response)),
                    Err(e) => match e {
                        Error::Rpc(RpcError::ElementNotFound { .. }) => Ok(None),
                        _ => Err(e),
                    },
                },
            )
            .filter_map(Result::transpose)
            .collect::<Result<Vec<ElementsResponseSingle>>>()?,
    ))
}

fn get_element_response(
    state: &web::Data<State>,
    element: Element,
    include_spent: bool,
) -> Result<ElementsResponseSingle> {
    match get_element_response_unspent(state, element) {
        Ok(resp) => Ok(resp),
        Err(e) => match e {
            Error::Rpc(RpcError::ElementNotFound { .. }) if include_spent => {
                get_element_response_from_history(state, element)
            }
            _ => Err(e),
        },
    }
}

fn get_element_response_unspent(
    state: &web::Data<State>,
    element: Element,
) -> Result<ElementsResponseSingle> {
    let notes_tree = state.node.notes_tree().read();
    let tree = notes_tree.tree();
    let meta = tree
        .get(element)
        .ok_or(RpcError::ElementNotFound(ElementData { element }))?;

    let Some(block) = state.node.get_block(meta.inserted_in.into())? else {
        return Err(Error::BlockNotFound {
            block: meta.inserted_in.into(),
        });
    };
    let block = block.into_block();

    let txn = block
        .content
        .state
        .txns
        .iter()
        .find(|txn| txn.public_inputs.commitments().contains(&element))
        .ok_or(Error::ElementNotInTxn {
            element,
            block_height: block.block_height(),
        })?;

    Ok(ElementsResponseSingle {
        element,
        height: meta.inserted_in,
        root_hash: block.content.state.root_hash,
        txn_hash: txn.hash(),
        spent: false,
    })
}

fn get_element_response_from_history(
    state: &web::Data<State>,
    element: Element,
) -> Result<ElementsResponseSingle> {
    let Some(info) = state.node.get_element_seen_info(element)? else {
        return Err(RpcError::ElementNotFound(ElementData { element }).into());
    };

    let Some(block) = state.node.get_block_by_hash(info.output_block_hash)? else {
        return Err(Error::BlockNotFound {
            block: info.output_height,
        });
    };
    let block = block.into_block();

    let txn = block
        .content
        .state
        .txns
        .iter()
        .find(|txn| txn.public_inputs.commitments().contains(&element))
        .ok_or(Error::ElementNotInTxn {
            element,
            block_height: info.output_height,
        })?;

    Ok(ElementsResponseSingle {
        element,
        height: info.output_height.0, // Use the height when the element was added to tree
        root_hash: block.content.state.root_hash,
        txn_hash: txn.hash(),
        spent: info.spent,
    })
}
