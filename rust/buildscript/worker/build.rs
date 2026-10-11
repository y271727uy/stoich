fn main() {
    println!("cargo:rerun-if-changed=../proto/stoich.proto");
    prost_build::compile_protos(&["../proto/stoich.proto"], &["../proto"]).expect("compile stoich.proto");
}
