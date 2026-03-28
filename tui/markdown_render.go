package tui

// RenderMarkdownDefault renders markdown using the default renderer configuration.
func RenderMarkdownDefault(content string, width int) string {
	return renderMarkdown(content, width)
}

// RenderMarkdownThinking renders markdown using the thinking renderer configuration.
func RenderMarkdownThinking(content string, width int) string {
	return renderMarkdown(content, width, ThinkingOption())
}
