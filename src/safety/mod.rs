pub fn sanitize_tool_output(output: &str) -> String {
    output.replace('\0', "")
}
