use reqwest::blocking;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::process;
use v8;

fn main() {
    let args: Vec<String> = env::args().collect();

    let params = Params::new(&args).unwrap_or_else(|err| {
        println!("{}", err);
        process::exit(1);
    });

    run(params);
}

fn run(params: Params) {
    let content = fs::read_to_string(&params.filename).expect("File could not be read.");

    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();

    let isolate = &mut v8::Isolate::new(Default::default());
    let handle_scope = &mut v8::HandleScope::new(isolate);
    let context = v8::Context::new(handle_scope);
    let scope = &mut v8::ContextScope::new(handle_scope, context);

    // Expose the Rust logging function to JavaScript
    let global = context.global(scope);
    let console_key = v8::String::new(scope, "console").unwrap();
    let console_obj = v8::Object::new(scope);
    let log_key = v8::String::new(scope, "log").unwrap();

    let log_callback = v8::FunctionTemplate::new(scope, log_handler);
    let log_function = log_callback.get_function(scope).unwrap();
    console_obj.set(scope, log_key.into(), log_function.into());

    global.set(scope, console_key.into(), console_obj.into());

    let code = v8::String::new(scope, &content).unwrap();
    let script = v8::Script::compile(scope, code, None).unwrap();
    script.run(scope).unwrap();

    // Expose Rust function to JavaScript
    // Create a FunctionTemplate and get the function
    let send_request_callback = v8::FunctionTemplate::new(scope, send_request_handler);
    let send_request_fn = send_request_callback.get_function(scope).unwrap();

    // Set the function in the global object
    {
        let global = context.global(scope);
        let key = v8::String::new(scope, "request").unwrap().into();
        global.set(scope, key, send_request_fn.into());
    }

    let init_code = v8::String::new(scope, "const instance = new source.default();").unwrap();
    let script = v8::Script::compile(scope, init_code, None).unwrap();
    script.run(scope).unwrap();

    let function_name: String;

    match params.option.as_str() {
        "--discover" => function_name = "discover()".to_string(),
        "--search" => match &params.url {
            Some(value) => {
                function_name = format!("search('{}')", value);
            }
            None => {
                println!("URL is required for --search option.");
                process::exit(1);
            }
        },
        "--info" => match &params.url {
            Some(value) => {
                function_name = format!("info('{}')", value);
            }
            None => {
                println!("URL is required for --info option.");
                process::exit(1);
            }
        },
        "--media" => match &params.url {
            Some(value) => {
                function_name = format!("media('{}')", value);
            }
            None => {
                println!("URL is required for --media option.");
                process::exit(1);
            }
        },
        "--servers" => match &params.url {
            Some(value) => {
                function_name = format!("servers('{}')", value);
            }
            None => {
                println!("URL is required for --servers option.");
                process::exit(1);
            }
        },
        "--sources" => match &params.url {
            Some(value) => {
                function_name = format!("sources('{}')", value);
            }
            None => {
                println!("URL is required for --sources option.");
                process::exit(1);
            }
        },
        _ => {
            println!("No option found.");
            process::exit(1);
        }
    }

    let async_function_code = v8::String::new(
        scope,
        format!(
            "
            new Promise((resolve, reject) => {{
                instance.{}.then(data => {{
                    resolve(JSON.stringify(data));
                }}).catch(error => {{
                    reject(error);
                }});
            }});
            ",
            function_name
        )
        .as_str(),
    )
    .unwrap();

    let script = v8::Script::compile(scope, async_function_code, None).unwrap();
    let result = script.run(scope).unwrap();
    let resolver = v8::PromiseResolver::new(scope).unwrap();
    let promise = resolver.get_promise(scope);

    resolver.resolve(scope, result);
    let result = promise.result(scope);

    let maybe_value = result.to_string(scope);
    if let Some(value) = maybe_value {
        let value_str = value.to_string(scope).unwrap();
        println!("{}", value_str.to_rust_string_lossy(scope));
    } else {
        println!("Promise did not resolve to a value.");
    }
}

fn log_handler(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    let message = args
        .get(0)
        .to_string(scope)
        .unwrap()
        .to_rust_string_lossy(scope);
    println!("JavaScript console.log: {}", message);
}

fn send_request_handler(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut return_value: v8::ReturnValue,
) {
    println!("request handler called.");
    let url = args.get(0).to_string(scope).unwrap();
    let method = args.get(1).to_string(scope).unwrap();

    // Simulate asynchronous operation (e.g., making an HTTP request)
    let response = send_request_async(
        url.to_rust_string_lossy(scope),
        method.to_rust_string_lossy(scope),
    );

    // Create the JavaScript object representing the response
    let v8_response = create_v8_response_object(scope, &response);

    // Set the return value of the JavaScript function
    return_value.set(v8_response.into());
}

fn send_request_async(url: String, method: String) -> Response {
    // Create a client
    let client = reqwest::blocking::Client::new();

    // Perform the request based on the method
    let result = match method.as_str() {
        "GET" => client.get(&url).send(),
        "POST" => client.post(&url).send(),
        _ => panic!("Unsupported method: {}", method),
    };

    match result {
        Ok(response) => {
            let status_code = response.status().as_u16() as i32;
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();

            let mut headers = HashMap::new();
            for (key, value) in response.headers().iter() {
                let key_string = key.to_string();
                let value_string = value.to_str().unwrap_or("").to_string();
                headers.insert(key_string, value_string);
            }

            // Now extract the body after all headers and status code are extracted
            let body = response.text().unwrap_or_default();

            Response {
                status_code,
                body,
                content_type,
                headers,
            }
        }
        Err(e) => {
            println!("Request failed: {}", e);
            Response {
                status_code: 500,
                body: "Internal Server Error".to_string(),
                content_type: "text/plain".to_string(),
                headers: HashMap::new(),
            }
        }
    }
}
#[derive(Debug)]
struct Response {
    status_code: i32,
    body: String,
    content_type: String,
    headers: HashMap<String, String>,
}

fn create_v8_response_object<'a>(
    scope: &mut v8::HandleScope<'a>,
    response: &Response,
) -> v8::Local<'a, v8::Object> {
    // Create a function template for the Response class
    let response_template = v8::FunctionTemplate::new(scope, response_constructor);

    // Get the function constructor from the template
    let constructor = response_template.get_function(scope).unwrap();

    // Create an empty object instance for the Response class
    let obj = constructor.new_instance(scope, &[]).unwrap();

    // Set properties on the instance
    let status_code_key = v8::String::new(scope, "statusCode").unwrap();
    let status_code_value = v8::Integer::new(scope, response.status_code);
    obj.set(scope, status_code_key.into(), status_code_value.into());

    let body_key = v8::String::new(scope, "body").unwrap();
    let body_value = v8::String::new(scope, &response.body).unwrap();
    obj.set(scope, body_key.into(), body_value.into());

    let content_type_key = v8::String::new(scope, "contentType").unwrap();
    let content_type_value = v8::String::new(scope, &response.content_type).unwrap();
    obj.set(scope, content_type_key.into(), content_type_value.into());

    let headers_key = v8::String::new(scope, "headers").unwrap();
    let headers_obj = v8::Object::new(scope);
    for (key, value) in &response.headers {
        let v8_key = v8::String::new(scope, key).unwrap();
        let v8_value = v8::String::new(scope, value).unwrap();
        headers_obj.set(scope, v8_key.into(), v8_value.into());
    }
    obj.set(scope, headers_key.into(), headers_obj.into());

    obj
}

// Constructor for the Response class
fn response_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut return_value: v8::ReturnValue,
) {
    // Create a new JavaScript object instance
    let obj = v8::Object::new(scope);

    // Set properties on the instance
    let status_code_key = v8::String::new(scope, "statusCode").unwrap();
    obj.set(scope, status_code_key.into(), args.get(0));

    let body_key = v8::String::new(scope, "body").unwrap();
    obj.set(scope, body_key.into(), args.get(1));

    let content_type_key = v8::String::new(scope, "contentType").unwrap();
    obj.set(scope, content_type_key.into(), args.get(2));

    let headers_key = v8::String::new(scope, "headers").unwrap();
    obj.set(scope, headers_key.into(), args.get(3));

    // Set the return value to the created object
    return_value.set(obj.into());
}

struct Params {
    filename: String,
    option: String,
    url: Option<String>,
}

impl Params {
    fn new(args: &[String]) -> Result<Params, &str> {
        if args.len() < 3 {
            return Err("usage: chouten <filename> <option> <url?>");
        }
        let filename = args[1].clone();
        let option = args[2].clone();

        if option != "--discover" && args.len() != 4 {
            return Err("usage: chouten <filename> <option> <url?>");
        }

        let url: Option<String> = args.get(3).cloned();

        Ok(Params {
            filename,
            option,
            url,
        })
    }
}
