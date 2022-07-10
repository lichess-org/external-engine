fn main() {
	println!("cargo:rerun-if-changed=Cargo.toml");
	println!("cargo:rerun-if-changed=favicon.ico");
	winres::WindowsResource::new()
	    .set_icon("favicon.ico")
		.compile()
		.expect("winres");
}