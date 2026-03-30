package tui

import (
	"sync"

	glamouransi "charm.land/glamour/v2/ansi"
	"github.com/alecthomas/chroma"
	"github.com/alecthomas/chroma/styles"
)

const thinkingCodeBlockThemeName = "hh-thinking-muted"

var markdownChromaThemesMu sync.Mutex

func registerCodeBlockTheme(name string, cfg *glamouransi.Chroma) {
	if cfg == nil || name == "" {
		return
	}

	markdownChromaThemesMu.Lock()
	defer markdownChromaThemesMu.Unlock()

	styles.Registry[name] = chroma.MustNewStyle(name,
		chroma.StyleEntries{
			chroma.Text:                chromaStyleFromPrimitive(cfg.Text),
			chroma.Error:               chromaStyleFromPrimitive(cfg.Error),
			chroma.Comment:             chromaStyleFromPrimitive(cfg.Comment),
			chroma.CommentPreproc:      chromaStyleFromPrimitive(cfg.CommentPreproc),
			chroma.Keyword:             chromaStyleFromPrimitive(cfg.Keyword),
			chroma.KeywordReserved:     chromaStyleFromPrimitive(cfg.KeywordReserved),
			chroma.KeywordNamespace:    chromaStyleFromPrimitive(cfg.KeywordNamespace),
			chroma.KeywordType:         chromaStyleFromPrimitive(cfg.KeywordType),
			chroma.Operator:            chromaStyleFromPrimitive(cfg.Operator),
			chroma.Punctuation:         chromaStyleFromPrimitive(cfg.Punctuation),
			chroma.Name:                chromaStyleFromPrimitive(cfg.Name),
			chroma.NameBuiltin:         chromaStyleFromPrimitive(cfg.NameBuiltin),
			chroma.NameTag:             chromaStyleFromPrimitive(cfg.NameTag),
			chroma.NameAttribute:       chromaStyleFromPrimitive(cfg.NameAttribute),
			chroma.NameClass:           chromaStyleFromPrimitive(cfg.NameClass),
			chroma.NameConstant:        chromaStyleFromPrimitive(cfg.NameConstant),
			chroma.NameDecorator:       chromaStyleFromPrimitive(cfg.NameDecorator),
			chroma.NameException:       chromaStyleFromPrimitive(cfg.NameException),
			chroma.NameFunction:        chromaStyleFromPrimitive(cfg.NameFunction),
			chroma.NameOther:           chromaStyleFromPrimitive(cfg.NameOther),
			chroma.Literal:             chromaStyleFromPrimitive(cfg.Literal),
			chroma.LiteralNumber:       chromaStyleFromPrimitive(cfg.LiteralNumber),
			chroma.LiteralDate:         chromaStyleFromPrimitive(cfg.LiteralDate),
			chroma.LiteralString:       chromaStyleFromPrimitive(cfg.LiteralString),
			chroma.LiteralStringEscape: chromaStyleFromPrimitive(cfg.LiteralStringEscape),
			chroma.GenericDeleted:      chromaStyleFromPrimitive(cfg.GenericDeleted),
			chroma.GenericEmph:         chromaStyleFromPrimitive(cfg.GenericEmph),
			chroma.GenericInserted:     chromaStyleFromPrimitive(cfg.GenericInserted),
			chroma.GenericStrong:       chromaStyleFromPrimitive(cfg.GenericStrong),
			chroma.GenericSubheading:   chromaStyleFromPrimitive(cfg.GenericSubheading),
			chroma.Background:          chromaStyleFromPrimitive(cfg.Background),
		})
}

func chromaStyleFromPrimitive(style glamouransi.StylePrimitive) string {
	s := ""
	if style.Color != nil {
		s = *style.Color
	}
	if style.BackgroundColor != nil {
		if s != "" {
			s += " "
		}
		s += "bg:" + *style.BackgroundColor
	}
	if style.Italic != nil && *style.Italic {
		if s != "" {
			s += " "
		}
		s += "italic"
	}
	if style.Bold != nil && *style.Bold {
		if s != "" {
			s += " "
		}
		s += "bold"
	}
	if style.Underline != nil && *style.Underline {
		if s != "" {
			s += " "
		}
		s += "underline"
	}
	return s
}
