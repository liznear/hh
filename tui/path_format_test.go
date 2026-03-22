package tui

import "testing"

func TestBeautifySidebarPath_ReplacesHomeAndCompacts(t *testing.T) {
	home := "/home/near"
	path := "/home/near/projects/a_folder/b_folder/c_folder/d_folder/file.txt"

	got := beautifySidebarPath(path, home)
	want := "~/p/a/b/c/d/file.txt"
	if got != want {
		t.Fatalf("beautifySidebarPath() = %q, want %q", got, want)
	}
}

func TestBeautifyToolPath_RemovesCwdPrefixAndCompacts(t *testing.T) {
	cwd := "/work/repo"
	path := "/work/repo/a_folder/b_folder/c_folder/d_folder/file.txt"

	got := beautifyToolPath(path, cwd)
	want := "a/b/c/d/file.txt"
	if got != want {
		t.Fatalf("beautifyToolPath() = %q, want %q", got, want)
	}
}

func TestBeautifyToolPath_DoesNotTrimSiblingPrefix(t *testing.T) {
	cwd := "/work/repo"
	path := "/work/repository/file.txt"

	got := beautifyToolPath(path, cwd)
	want := "/work/repository/file.txt"
	if got != want {
		t.Fatalf("beautifyToolPath() = %q, want %q", got, want)
	}
}
