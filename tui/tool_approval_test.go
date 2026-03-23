package tui

import "testing"

func TestBashRuleMatches(t *testing.T) {
	tests := []struct {
		name    string
		rule    string
		command string
		want    bool
	}{
		{name: "base wildcard", rule: "Bash(ls:*)", command: "ls -al", want: true},
		{name: "prefix wildcard", rule: "Bash(go test:*)", command: "go test ./...", want: true},
		{name: "exact", rule: "Bash(go run update.go)", command: "go run update.go", want: true},
		{name: "exact mismatch", rule: "Bash(go run update.go)", command: "go run other.go", want: false},
		{name: "prefix mismatch", rule: "Bash(ls:*)", command: "lsof", want: false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := bashRuleMatches(tt.rule, tt.command); got != tt.want {
				t.Fatalf("bashRuleMatches(%q, %q) = %v, want %v", tt.rule, tt.command, got, tt.want)
			}
		})
	}
}

func TestIsWithinDir(t *testing.T) {
	if !isWithinDir("/tmp/work/a.txt", "/tmp/work") {
		t.Fatal("expected path to be within directory")
	}
	if isWithinDir("/tmp/other/a.txt", "/tmp/work") {
		t.Fatal("expected path to be outside directory")
	}
}
