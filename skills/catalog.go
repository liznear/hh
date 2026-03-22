package skills

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
)

const skillFileName = "SKILL.md"

type Entry struct {
	Name        string
	Description string
	Path        string
	Location    string
	Frontmatter string
	Content     string
}

type Catalog struct {
	entries []Entry
	byName  map[string]int
}

func (c Catalog) IsEmpty() bool {
	return len(c.entries) == 0
}

func (c Catalog) Entries() []Entry {
	out := make([]Entry, len(c.entries))
	copy(out, c.entries)
	return out
}

func (c Catalog) SkillByName(name string) (Entry, bool) {
	if strings.TrimSpace(name) == "" {
		return Entry{}, false
	}
	idx, ok := c.byName[strings.ToLower(strings.TrimSpace(name))]
	if !ok || idx < 0 || idx >= len(c.entries) {
		return Entry{}, false
	}
	return c.entries[idx], true
}

func (c Catalog) PromptFrontmatterBlock() string {
	if len(c.entries) == 0 {
		return ""
	}

	var b strings.Builder
	b.WriteString("Use the skill tool to load a skill when a task matches its description.\n")
	b.WriteString("<available_skills>\n")
	for _, entry := range c.entries {
		b.WriteString("  <skill>\n")
		b.WriteString("    <name>")
		b.WriteString(xmlEscape(entry.Name))
		b.WriteString("</name>\n")
		if strings.TrimSpace(entry.Description) != "" {
			b.WriteString("    <description>")
			b.WriteString(xmlEscape(entry.Description))
			b.WriteString("</description>\n")
		}
		b.WriteString("    <location>")
		b.WriteString(xmlEscape(entry.Location))
		b.WriteString("</location>\n")
		if strings.TrimSpace(entry.Frontmatter) != "" {
			b.WriteString("    <frontmatter>\n")
			for _, line := range strings.Split(entry.Frontmatter, "\n") {
				b.WriteString("      ")
				b.WriteString(xmlEscape(line))
				b.WriteByte('\n')
			}
			b.WriteString("    </frontmatter>\n")
		}
		b.WriteString("  </skill>\n")
	}
	b.WriteString("</available_skills>")

	return b.String()
}

func DefaultDir() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("resolve home dir: %w", err)
	}
	return filepath.Join(home, ".agents", "skills"), nil
}

func LoadDefaultCatalog() (Catalog, error) {
	defaultCatalogOnce.Do(func() {
		dir, err := DefaultDir()
		if err != nil {
			defaultCatalogErr = err
			return
		}
		defaultCatalog, defaultCatalogErr = LoadDir(dir)
	})
	return defaultCatalog, defaultCatalogErr
}

func LoadDir(dir string) (Catalog, error) {
	info, err := os.Stat(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return Catalog{entries: nil, byName: map[string]int{}}, nil
		}
		return Catalog{}, fmt.Errorf("stat skills dir: %w", err)
	}
	if !info.IsDir() {
		return Catalog{}, fmt.Errorf("skills path is not a directory: %s", dir)
	}

	items, err := os.ReadDir(dir)
	if err != nil {
		return Catalog{}, fmt.Errorf("read skills dir: %w", err)
	}

	sort.Slice(items, func(i, j int) bool {
		return strings.ToLower(items[i].Name()) < strings.ToLower(items[j].Name())
	})

	entries := make([]Entry, 0, len(items))
	byName := make(map[string]int, len(items)*2)

	for _, item := range items {
		if !item.IsDir() {
			continue
		}

		skillPath := filepath.Join(dir, item.Name(), skillFileName)
		raw, err := os.ReadFile(skillPath)
		if err != nil {
			if os.IsNotExist(err) {
				continue
			}
			return Catalog{}, fmt.Errorf("read skill file %s: %w", skillPath, err)
		}

		frontmatter, fields := extractFrontmatter(string(raw))
		skillName := firstNonEmpty(fields["name"], item.Name())
		description := strings.TrimSpace(fields["description"])

		absSkillPath, err := filepath.Abs(skillPath)
		if err != nil {
			absSkillPath = skillPath
		}

		entry := Entry{
			Name:        skillName,
			Description: description,
			Path:        absSkillPath,
			Location:    "file://" + filepath.ToSlash(absSkillPath),
			Frontmatter: frontmatter,
			Content:     string(raw),
		}

		idx := len(entries)
		entries = append(entries, entry)
		byName[strings.ToLower(skillName)] = idx
		byName[strings.ToLower(item.Name())] = idx
	}

	return Catalog{entries: entries, byName: byName}, nil
}

func extractFrontmatter(content string) (string, map[string]string) {
	if !strings.HasPrefix(content, "---\n") && !strings.HasPrefix(content, "---\r\n") {
		return "", map[string]string{}
	}

	startDelimiterLen := len("---\n")
	if strings.HasPrefix(content, "---\r\n") {
		startDelimiterLen = len("---\r\n")
	}

	rest := content[startDelimiterLen:]
	end := strings.Index(rest, "\n---")
	if end < 0 {
		return "", map[string]string{}
	}

	raw := strings.TrimSpace(rest[:end])
	fields := parseFrontmatterFields(raw)
	return raw, fields
}

func parseFrontmatterFields(raw string) map[string]string {
	out := map[string]string{}
	if strings.TrimSpace(raw) == "" {
		return out
	}

	for _, line := range strings.Split(raw, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		key, value, ok := strings.Cut(line, ":")
		if !ok {
			continue
		}
		key = strings.ToLower(strings.TrimSpace(key))
		if key == "" {
			continue
		}
		value = strings.TrimSpace(value)
		value = strings.Trim(value, "\"'")
		out[key] = value
	}

	return out
}

func firstNonEmpty(values ...string) string {
	for _, v := range values {
		if strings.TrimSpace(v) != "" {
			return strings.TrimSpace(v)
		}
	}
	return ""
}

func xmlEscape(s string) string {
	r := strings.NewReplacer(
		"&", "&amp;",
		"<", "&lt;",
		">", "&gt;",
		"\"", "&quot;",
		"'", "&apos;",
	)
	return r.Replace(s)
}

var (
	defaultCatalogOnce sync.Once
	defaultCatalog     Catalog
	defaultCatalogErr  error
)
