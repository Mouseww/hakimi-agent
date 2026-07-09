//! Tracing 使用示例

use hakimi_metrics::{SpanContext, SpanEvent, Tracer};

fn main() {
    // 创建全局 Tracer
    let tracer = Tracer::new();

    // 示例 1: 基本 Span
    basic_span_example(&tracer);

    // 示例 2: 嵌套 Span
    nested_span_example(&tracer);

    // 示例 3: 使用 SpanContext 自动管理
    context_example(&tracer);

    // 查看统计
    let stats = tracer.stats();
    println!("\n=== Tracer 统计 ===");
    println!("总 Span 数: {}", stats.total_spans);
    println!("总 Trace 数: {}", stats.total_traces);
    println!("平均每 Trace Span 数: {:.2}", stats.avg_spans_per_trace);
}

fn basic_span_example(tracer: &Tracer) {
    println!("=== 基本 Span 示例 ===");

    let mut span = tracer.start_trace("basic_operation");
    span.add_tag("user_id", "12345");
    span.add_tag("operation", "data_processing");

    // 模拟操作
    std::thread::sleep(std::time::Duration::from_millis(100));

    span.add_event(SpanEvent::new("data_loaded").with_attribute("rows", "1000"));

    std::thread::sleep(std::time::Duration::from_millis(50));

    span.finish();
    tracer.record_span(span.clone());

    println!("Trace ID: {}", span.trace_id);
    println!("Span ID: {}", span.span_id);
    println!("持续时间: {:?}", span.duration());
}

fn nested_span_example(tracer: &Tracer) {
    println!("\n=== 嵌套 Span 示例 ===");

    let mut parent = tracer.start_trace("request_handler");
    parent.add_tag("endpoint", "/api/users");
    let trace_id = parent.trace_id;

    // 子操作 1: 数据库查询
    let mut db_span = parent.child("database_query");
    db_span.add_tag("query", "SELECT * FROM users");
    std::thread::sleep(std::time::Duration::from_millis(30));
    db_span.finish();
    tracer.record_span(db_span);

    // 子操作 2: 数据处理
    let mut process_span = parent.child("data_processing");
    process_span.add_tag("rows", "50");
    std::thread::sleep(std::time::Duration::from_millis(20));
    process_span.finish();
    tracer.record_span(process_span);

    parent.finish();
    tracer.record_span(parent);

    // 查看整个 Trace
    let spans = tracer.get_trace_spans(trace_id);
    println!("Trace {} 包含 {} 个 Span:", trace_id, spans.len());
    for span in spans {
        let indent = if span.parent_span_id.is_some() {
            "  └─ "
        } else {
            ""
        };
        println!(
            "{}{}  ({}ms)",
            indent,
            span.name,
            span.duration().unwrap().as_millis()
        );
    }
}

fn context_example(tracer: &Tracer) {
    println!("\n=== SpanContext 示例 ===");

    let span = tracer.start_trace("automatic_finish");
    let mut ctx = SpanContext::new(span);

    // 自动追踪作用域
    {
        ctx.span_mut().unwrap().add_tag("auto", "true");
        std::thread::sleep(std::time::Duration::from_millis(50));
    } // ctx drop 时自动调用 finish

    let finished_span = ctx.finish().unwrap();
    tracer.record_span(finished_span.clone());

    println!("Span {} 自动完成", finished_span.name);
    println!("持续时间: {:?}", finished_span.duration());
}
