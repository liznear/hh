package tui

import (
	"path/filepath"
	"strings"
	"unicode/utf8"
)

const compactPathThreshold = 30

func beautifySidebarPath(path string, home string) string {
	path = normalizeDisplayPath(path)
	if path == "" {
		return path
	}

	home = normalizeDisplayPath(home)
	if home != "" {
		switch {
		case path == home:
			path = "~"
		case strings.HasPrefix(path, home+"/"):
			path = "~" + strings.TrimPrefix(path, home)
		}
	}

	return compactPathFolders(path, compactPathThreshold)
}

func beautifyToolPath(path string, cwd string) string {
	path = normalizeDisplayPath(path)
	if path == "" {
		return path
	}

	cwd = normalizeDisplayPath(cwd)
	if cwd != "" {
		switch {
		case path == cwd:
			path = "."
		case strings.HasPrefix(path, cwd+"/"):
			path = strings.TrimPrefix(path, cwd+"/")
		}
	}

	return compactPathFolders(path, compactPathThreshold)
}

func normalizeDisplayPath(path string) string {
	path = strings.TrimSpace(path)
	if path == "" {
		return ""
	}
	return filepath.ToSlash(filepath.Clean(path))
}

func compactPathFolders(path string, threshold int) string {
	if threshold <= 0 || utf8.RuneCountInString(path) <= threshold {
		return path
	}

	prefix := ""
	suffix := ""
	trimmed := path

	if strings.HasPrefix(trimmed, "~/") {
		prefix = "~/"
		trimmed = strings.TrimPrefix(trimmed, "~/")
	} else if strings.HasPrefix(trimmed, "/") {
		prefix = "/"
		trimmed = strings.TrimPrefix(trimmed, "/")
	}

	if strings.HasSuffix(trimmed, "/") {
		suffix = "/"
		trimmed = strings.TrimSuffix(trimmed, "/")
	}

	parts := strings.Split(trimmed, "/")
	if len(parts) <= 1 {
		return path
	}

	for i := 0; i < len(parts)-1; i++ {
		part := parts[i]
		if part == "" || part == "." || part == ".." {
			continue
		}
		r, _ := utf8.DecodeRuneInString(part)
		if r == utf8.RuneError {
			continue
		}
		parts[i] = string(r)
	}

	return prefix + strings.Join(parts, "/") + suffix
}
