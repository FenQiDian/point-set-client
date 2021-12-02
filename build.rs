extern crate protoc_rust;

fn main() {
    bindgen::builder()
        .header("kcp/ikcp.h")
        .allowlist_type("IQUEUEHEAD")
        .allowlist_type("IKCPSEG")
        .allowlist_type("IKCPCB")
        .allowlist_function("ikcp_create")
        .allowlist_function("ikcp_release")
        .allowlist_function("ikcp_setoutput")
        .allowlist_function("ikcp_recv")
        .allowlist_function("ikcp_send")
        .allowlist_function("ikcp_update")
        .allowlist_function("ikcp_check")
        .allowlist_function("ikcp_input")
        .allowlist_function("ikcp_flush")
        .allowlist_function("ikcp_peeksize")
        .allowlist_function("ikcp_setmtu")
        .allowlist_function("ikcp_wndsize")
        .allowlist_function("ikcp_waitsnd")
        .allowlist_function("ikcp_nodelay")
        .allowlist_function("ikcp_allocator")
        .allowlist_function("ikcp_getconv")
        .generate()
        .unwrap()
        .write_to_file("src/ikcp.rs")
        .unwrap();

    cc::Build::new()
        .include("kcp")
        .file("kcp/ikcp.c")
        .compile("kcp");

    protoc_rust::Codegen::new()
        .out_dir("./src")
        .inputs(&["./src/message.proto"])
        .include("./src")
        .run()
        .unwrap();
}
