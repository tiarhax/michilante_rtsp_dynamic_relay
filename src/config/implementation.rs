use super::interface::{Camera, CameraConfigRepository};
pub struct AWSCameraConfigRepository {
    client: aws_sdk_dynamodb::Client,
    table_name: String,
    partition_key: String,
}

impl AWSCameraConfigRepository {
    pub async fn new(
        config: aws_config::SdkConfig,
        table_name: String,
        partition_key: String,
    ) -> Self {
        return Self {
            client: aws_sdk_dynamodb::Client::new(&config),
            partition_key,
            table_name
        };
    }
}
#[derive(Debug, Clone)]
pub struct DyunamoDBError {
    pub debug_message: String,
}
#[derive(Debug, Clone)]
pub enum ListingCamerasError {
    DynamoDBError(DyunamoDBError),
}

impl CameraConfigRepository for AWSCameraConfigRepository {
    type Error = ListingCamerasError;

    async fn list_all(&self) -> Result<Vec<Camera>, Self::Error> {
        let table_name = self.table_name.clone();
        let partition_key = self.partition_key.clone();

        let result = self
            .client
            .query()
            .table_name(table_name)
            .key_condition_expression("#partitionKey = :pkval")
            .expression_attribute_names("#partitionKey", "partitionKey")
            .expression_attribute_values(
                ":pkval",
                aws_sdk_dynamodb::types::AttributeValue::S(partition_key.to_string()),
            )
            .send()
            .await
            .map_err(|err| {
                ListingCamerasError::DynamoDBError(DyunamoDBError {
                    debug_message: format!("DynamoDB query failed: {:?}", err),
                })
            })?;

        let cameras = result
            .items
            .unwrap_or_default()
            .into_iter()
            .filter_map(|item| {
                let camera_id = item.get("sortKey")?.as_s().ok()?.to_string();
                let source_url = item.get("url")?.as_s().ok()?.to_string();

                Some(Camera {
                    id: camera_id,
                    source_url,
                })
            })
            .collect();

        Ok(cameras)
    }
}
