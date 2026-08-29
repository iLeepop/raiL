use std::{collections::HashMap, error::Error, future::Future, pin::Pin, sync::Arc};

use llm::{ChatCompletionTool, ChatCompletionTools, FunctionObject};
use serde::de::DeserializeOwned;

/// 工具执行结果:JSON 值或错误
pub type ToolResult = Result<serde_json::Value, Box<dyn Error>>;

/// 擦除后的 handler:接收原始 JSON 参数,返回异步结果
type ErasedHandler = Arc<
    dyn Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = ToolResult> + Send>> + Send + Sync,
>;

/// 工具的 JSON Schema 参数描述,用于指导 LLM 生成调用参数
#[derive(Debug, Clone)]
pub struct ToolParameters(pub serde_json::Value);

impl ToolParameters {
    pub fn new(schema: serde_json::Value) -> Self {
        Self(schema)
    }
}

/// 调用工具时的错误
#[derive(Debug)]
pub enum ToolError {
    /// 注册中心里没有这个名字的工具
    NotFound(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::NotFound(name) => write!(f, "tool not found: {name}"),
        }
    }
}

impl std::error::Error for ToolError {}

/// 注册中心里的一个工具
#[derive(Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: ToolParameters,
    handler: ErasedHandler,
}

/// 工具注册中心:注册时擦除参数类型,调用时在边界恢复
#[derive(Default, Clone)]
pub struct ToolRegister {
    tools: HashMap<String, Tool>,
}

impl ToolRegister {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个异步工具。
    ///
    /// `Args` 是工具的参数类型,`handler` 接收强类型参数;LLM 传入的
    /// JSON 参数会在调用边界反序列化为 `Args`,失败时返回 Err 而非 panic。
    pub fn register<Args, F, Fut>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: ToolParameters,
        handler: F,
    ) where
        Args: DeserializeOwned + 'static,
        F: Fn(Args) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ToolResult> + Send + 'static,
    {
        let name = name.into();
        let handler: ErasedHandler = Arc::new(
            move |raw| -> Pin<Box<dyn Future<Output = ToolResult> + Send>> {
                match serde_json::from_value::<Args>(raw) {
                    Ok(args) => Box::pin(handler(args)),
                    Err(e) => Box::pin(async move { Err(e.into()) }),
                }
            },
        );
        self.tools.insert(
            name.clone(),
            Tool {
                name,
                description: description.into(),
                parameters,
                handler,
            },
        );
    }

    pub fn get(&self, name: &str) -> Option<&Tool> {
        self.tools.get(name)
    }

    /// 转成发给 LLM 的 ChatCompletionTools 列表
    pub fn schemas(&self) -> Vec<ChatCompletionTools> {
        self.tools
            .values()
            .map(|t| {
                ChatCompletionTools::Function(ChatCompletionTool {
                    function: FunctionObject {
                        name: t.name.clone(),
                        description: Some(t.description.clone()),
                        parameters: Some(t.parameters.0.clone()),
                        strict: None,
                    },
                })
            })
            .collect()
    }

    /// 执行工具。反序列化或 handler 内部的错误作为 Err 返回,由调用方决定
    /// 是否回填给 LLM。
    pub async fn call(&self, name: &str, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        match self.tools.get(name) {
            Some(t) => Ok((t.handler)(args).await),
            None => Err(ToolError::NotFound(name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, serde::Deserialize)]
    struct CalcArgs {
        a: f64,
        b: f64,
    }

    fn calc_register() -> ToolRegister {
        let mut reg = ToolRegister::new();
        reg.register(
            "calc",
            "计算两数之和",
            ToolParameters::new(serde_json::json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" },
                },
                "required": ["a", "b"],
            })),
            |args: CalcArgs| async move { Ok(serde_json::json!({ "result": args.a + args.b })) },
        );
        reg
    }

    #[tokio::test]
    async fn roundtrip_success() {
        let reg = calc_register();
        let out = reg
            .call("calc", serde_json::json!({ "a": 1.0, "b": 2.0 }))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out, serde_json::json!({ "result": 3.0 }));
    }

    #[tokio::test]
    async fn bad_args_deserialize_to_err() {
        let reg = calc_register();
        // 缺 b 字段 → 反序列化失败,返回 Err 而不是 panic
        let res = reg
            .call("calc", serde_json::json!({ "a": 1.0 }))
            .await
            .unwrap();
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn unknown_tool_is_not_found() {
        let reg = calc_register();
        assert!(matches!(
            reg.call("nope", serde_json::json!({})).await,
            Err(ToolError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn handler_error_propagates() {
        let mut reg = ToolRegister::new();
        reg.register(
            "boom",
            "总是失败",
            ToolParameters::new(serde_json::json!({})),
            |_: serde_json::Value| async move {
                Err::<serde_json::Value, _>("handler exploded".into())
            },
        );
        let res = reg.call("boom", serde_json::json!({})).await.unwrap();
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn schemas_carry_name_and_parameters() {
        let reg = calc_register();
        let schemas = reg.schemas();
        assert_eq!(schemas.len(), 1);
        let s = serde_json::to_value(&schemas[0]).unwrap();
        assert_eq!(s["function"]["name"], "calc");
        assert_eq!(s["function"]["parameters"]["required"][0], "a");
    }
}
