zip:
	rm -f xbrabl04.zip
	zip -r xbrabl04.zip build.rs Cargo.toml documentation.pdf rust-toolchain.toml src/ documentation/ sdkconfig.defaults README.md
