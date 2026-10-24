use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=glsl/cde_border.vert");
    println!("cargo:rerun-if-changed=glsl/cde_border.frag");

    let compiler = shaderc::Compiler::new().expect("Failed to initialize shaderc compiler");
    let mut options = shaderc::CompileOptions::new().expect("Failed to initialize compile options");
    options.set_optimization_level(shaderc::OptimizationLevel::Performance);

    let glsl_to_compile = [
        (
            "glsl/cde_border.vert",
            "cde_border.vert.spv",
            shaderc::ShaderKind::Vertex,
        ),
        (
            "glsl/cde_border.frag",
            "cde_border.frag.spv",
            shaderc::ShaderKind::Fragment,
        ),
    ];

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    for (src_path, dest_name, kind) in &glsl_to_compile {
        // Read GLSL source code
        let source_code = std::fs::read_to_string(src_path)
            .unwrap_or_else(|_| panic!("Failed to read shader file: {}", src_path));

        // Compile GLSL source code to SPIR-V
        let artifact = compiler
            .compile_into_spirv(&source_code, *kind, src_path, "main", Some(&options))
            .unwrap_or_else(|e| panic!("Shader compilation error in {}: {}", src_path, e));

        // Save binary file into the target build output directory
        let mut file = File::create(out_dir.join(dest_name)).unwrap();
        file.write_all(artifact.as_binary_u8()).unwrap();
    }
}
