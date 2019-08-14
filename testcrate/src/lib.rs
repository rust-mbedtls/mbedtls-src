use std::os::raw::c_uint;

extern "C" {
    pub fn mbedtls_version_get_number() -> c_uint;
}

#[test]
fn version_works() {
    unsafe {
        println!("{:#x}", mbedtls_version_get_number());
        assert!(mbedtls_version_get_number() > 0);
    }
}
