use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("数据库错误: {0}")]
    Database(#[from] sqlx::Error),

    #[error("MongoDB 错误: {0}")]
    Mongo(#[from] mongodb::error::Error),

    #[error("加密错误: {0}")]
    Crypto(String),

    #[error("连接错误: {0}")]
    Connection(String),

    #[error("未找到: {0}")]
    NotFound(String),

    #[error("输入无效: {0}")]
    InvalidInput(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
