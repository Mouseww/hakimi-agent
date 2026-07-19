//! 过程宏实现 - 自动生成插件导出函数

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemStruct, Lit};

/// `#[hakimi_plugin]` 过程宏
///
/// 自动实现：
/// - 元数据导出函数 `__hakimi_plugin_metadata()`
/// - 初始化/执行/清理函数
///
/// # 示例
///
/// ```ignore
/// #[hakimi_plugin(
///     name = "example",
///     version = "1.0.0",
///     author = "You"
/// )]
/// pub struct ExamplePlugin;
/// ```
#[proc_macro_attribute]
pub fn hakimi_plugin(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let struct_name = &input.ident;

    // 解析属性参数
    let mut name = String::new();
    let mut version = String::new();
    let mut author = String::new();
    let mut description = String::new();

    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("name") {
            let value: Lit = meta.value()?.parse()?;
            if let Lit::Str(s) = value {
                name = s.value();
            }
        } else if meta.path.is_ident("version") {
            let value: Lit = meta.value()?.parse()?;
            if let Lit::Str(s) = value {
                version = s.value();
            }
        } else if meta.path.is_ident("author") {
            let value: Lit = meta.value()?.parse()?;
            if let Lit::Str(s) = value {
                author = s.value();
            }
        } else if meta.path.is_ident("description") {
            let value: Lit = meta.value()?.parse()?;
            if let Lit::Str(s) = value {
                description = s.value();
            }
        }
        Ok(())
    });

    parse_macro_input!(attr with parser);

    // 生成代码
    let expanded = quote! {
        #input

        // 元数据导出函数（运行时加载器会调用）
        #[no_mangle]
        pub extern "C" fn __hakimi_plugin_metadata(buf: *mut u8, buf_len: usize) -> i32 {
            use hakimi_plugin_sdk::PluginMetadata;

            let metadata = PluginMetadata {
                name: #name.to_string(),
                version: #version.to_string(),
                author: #author.to_string(),
                description: #description.to_string(),
            };

            let json = hakimi_plugin_sdk::serde_json::to_string(&metadata).unwrap();
            let bytes = json.as_bytes();

            if bytes.len() > buf_len {
                return -1; // 缓冲区不足
            }

            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
            }

            bytes.len() as i32
        }

        // 插件初始化
        #[no_mangle]
        pub extern "C" fn __hakimi_plugin_init() -> i32 {
            0 // 成功
        }

        // 插件执行（用户需实现 execute 方法）
        #[no_mangle]
        pub extern "C" fn __hakimi_plugin_execute(
            input_ptr: *const u8,
            input_len: usize,
            output_ptr: *mut u8,
            output_len: usize,
        ) -> i32 {
            let plugin = #struct_name;
            let ctx = hakimi_plugin_sdk::PluginContext::new();

            // 读取输入（如果需要）
            let _input_slice = if input_len > 0 {
                unsafe { std::slice::from_raw_parts(input_ptr, input_len) }
            } else {
                &[]
            };

            // 调用用户实现的 execute 方法
            let result = plugin.execute(&ctx);

            match result {
                Ok(output) => {
                    let bytes = output.as_bytes();
                    if bytes.len() > output_len {
                        return -1; // 缓冲区不足
                    }
                    unsafe {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output_ptr, bytes.len());
                    }
                    bytes.len() as i32
                }
                Err(_) => -1,
            }
        }

        // 插件清理
        #[no_mangle]
        pub extern "C" fn __hakimi_plugin_shutdown() -> i32 {
            0 // 成功
        }
    };

    TokenStream::from(expanded)
}
