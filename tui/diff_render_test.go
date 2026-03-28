package tui

import (
	"strings"
	"testing"

	"github.com/charmbracelet/x/ansi"
)

func TestRenderSplitDiff_ColumnsAndStyles(t *testing.T) {
	oldContent := `oldValue := map[string]int{"thisIsAVeryLongIdentifierThatShouldWrapAcrossColumn": 1}`
	newContent := `newValue := map[string]int{"thisIsAVeryLongIdentifierThatShouldWrapAcrossColumn": 2}`

	lines := RenderSplitDiff(oldContent, newContent, "sample.go", 64, DefaultTheme())
	if len(lines) < 2 {
		t.Fatalf("expected split diff output, got %d lines", len(lines))
	}

	hasHunkHeader := false
	rowCount := 0
	for _, line := range lines {
		plain := ansi.Strip(line)

		// Debug output
		t.Logf("Line: %q", plain)

		// Check for hunk header
		if strings.HasPrefix(plain, "@@") {
			hasHunkHeader = true
		}

		if !strings.Contains(plain, " │ ") {
			continue
		}
		parts := strings.SplitN(plain, " │ ", 2)
		if len(parts) != 2 {
			t.Fatalf("unexpected split for row %q", plain)
		}
		if ansi.StringWidth(parts[0]) != ansi.StringWidth(parts[1]) {
			t.Fatalf("expected equal column widths, left=%d right=%d row=%q", ansi.StringWidth(parts[0]), ansi.StringWidth(parts[1]), plain)
		}

		// Check that left side has delete marker and right side has insert marker
		leftHasDelete := strings.Contains(parts[0], "-") && strings.Contains(parts[0], "oldValue")
		rightHasInsert := strings.Contains(parts[1], "+") && strings.Contains(parts[1], "newValue")
		if leftHasDelete && rightHasInsert {
			rowCount++
		}
	}

	if !hasHunkHeader {
		t.Fatal("expected hunk header")
	}
	if rowCount == 0 {
		t.Fatal("expected side-by-side rows with delete/insert markers")
	}
}

func TestRenderSplitDiff_NoChanges(t *testing.T) {
	content := "same content"
	lines := RenderSplitDiff(content, content, "test.go", 64, DefaultTheme())
	if len(lines) != 1 || lines[0] != "(no changes)" {
		t.Fatalf("expected '(no changes)', got %v", lines)
	}
}

func TestRenderSplitDiff_MultipleHunks(t *testing.T) {
	oldContent := `package main

func oldFunction() {
    println("old")
}

func unchanged() {
    println("same")
}
`
	newContent := `package main

func newFunction() {
    println("new")
}

func unchanged() {
    println("same")
}
`
	lines := RenderSplitDiff(oldContent, newContent, "main.go", 80, DefaultTheme())
	if len(lines) < 3 {
		t.Fatalf("expected multiple lines, got %d", len(lines))
	}

	// Check for hunk header
	hasHunkHeader := false
	for _, line := range lines {
		plain := ansi.Strip(line)
		if strings.HasPrefix(plain, "@@") {
			hasHunkHeader = true
			break
		}
	}
	if !hasHunkHeader {
		t.Fatal("expected hunk header")
	}
}
