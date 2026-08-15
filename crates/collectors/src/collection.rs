use url::Url;

use crate::{
    CollectContext, CollectorBatch, CollectorError, CollectorHealth, CollectorItem, FetchResponse,
    HttpFetcher,
};

pub(crate) async fn collect_http(
    source: &str,
    fetcher: &dyn HttpFetcher,
    endpoint: &Url,
    context: &CollectContext,
    headers: &[(&str, &str)],
    parse: impl FnOnce(&FetchResponse) -> Result<Vec<CollectorItem>, CollectorError>,
) -> Result<CollectorBatch, CollectorError> {
    let response = fetcher
        .get(endpoint, context.cursor.as_ref(), headers)
        .await?;
    if response.not_modified {
        return Ok(CollectorBatch {
            source: source.into(),
            items: Vec::new(),
            health: CollectorHealth::healthy(),
            cursor: response.cursor,
            not_modified: true,
        });
    }
    let items = parse(&response)?;
    Ok(CollectorBatch {
        source: source.into(),
        items,
        health: CollectorHealth::healthy(),
        cursor: response.cursor,
        not_modified: false,
    })
}
