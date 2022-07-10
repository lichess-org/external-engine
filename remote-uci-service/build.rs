fn main() {
    println!("cargo:rerun-if-changed=favicon.ico");
	winres::WindowsResource::new()
	    .set_icon("favicon.ico")
		.set("ProductName", "External Engine")
		.set("CompanyName", "lichess.org")
		.compile()
		.expect("winres");
}