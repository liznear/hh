package tui

import (
	"encoding/json"
	"fmt"
	"reflect"
	"strconv"
	"strings"

	glamouransi "github.com/charmbracelet/glamour/ansi"
)

type rgbColor struct {
	r int
	g int
	b int
}

var defaultMutedBackground = rgbColor{r: 255, g: 255, b: 255}

// mutedStyleConfig returns a copy of style with foreground/background colors blended toward background.
// amount controls blending strength in [0,1].
func mutedStyleConfig(style glamouransi.StyleConfig, amount float64) glamouransi.StyleConfig {
	amount = clamp01(amount)
	if amount == 0 {
		return style
	}

	out := deepCopyStyleConfig(style)
	baseBackground := styleConfigBackground(out)
	muteStyleConfigColors(reflect.ValueOf(&out).Elem(), amount, baseBackground)
	return out
}

func deepCopyStyleConfig(style glamouransi.StyleConfig) glamouransi.StyleConfig {
	var out glamouransi.StyleConfig
	b, err := json.Marshal(style)
	if err != nil {
		return style
	}
	if err := json.Unmarshal(b, &out); err != nil {
		return style
	}
	return out
}

func styleConfigBackground(style glamouransi.StyleConfig) rgbColor {
	if style.Document.BackgroundColor != nil {
		if bg, ok := parseColor(*style.Document.BackgroundColor); ok {
			return bg
		}
	}
	return defaultMutedBackground
}

func muteStyleConfigColors(v reflect.Value, amount float64, inheritedBackground rgbColor) {
	if !v.IsValid() {
		return
	}

	switch v.Kind() {
	case reflect.Struct:
		t := v.Type()
		currentBackground := structBackground(v, inheritedBackground)
		for i := 0; i < v.NumField(); i++ {
			field := v.Field(i)
			fieldType := t.Field(i)
			if !field.CanSet() {
				continue
			}

			if isColorPointerField(fieldType.Name, field) {
				orig := field.Elem().String()
				color, ok := parseColor(orig)
				if !ok {
					continue
				}

				target := currentBackground
				if fieldType.Name == "BackgroundColor" {
					target = inheritedBackground
				}
				muted := blendColor(color, target, amount)
				mutedHex := fmt.Sprintf("#%02x%02x%02x", muted.r, muted.g, muted.b)
				field.Set(reflect.ValueOf(&mutedHex))
				continue
			}

			muteStyleConfigColors(field, amount, currentBackground)
		}
	case reflect.Ptr:
		if v.IsNil() {
			return
		}
		muteStyleConfigColors(v.Elem(), amount, inheritedBackground)
	}
}

func structBackground(v reflect.Value, fallback rgbColor) rgbColor {
	backgroundField := v.FieldByName("BackgroundColor")
	if !backgroundField.IsValid() || backgroundField.Kind() != reflect.Ptr || backgroundField.IsNil() {
		return fallback
	}
	if backgroundField.Type().Elem().Kind() != reflect.String {
		return fallback
	}
	bg, ok := parseColor(backgroundField.Elem().String())
	if !ok {
		return fallback
	}
	return bg
}

func isColorPointerField(name string, field reflect.Value) bool {
	if name != "Color" && name != "BackgroundColor" {
		return false
	}
	if field.Kind() != reflect.Ptr || field.IsNil() {
		return false
	}
	return field.Type().Elem().Kind() == reflect.String
}

func blendColor(color rgbColor, background rgbColor, amount float64) rgbColor {
	return rgbColor{
		r: clamp255(lerp(float64(color.r), float64(background.r), amount)),
		g: clamp255(lerp(float64(color.g), float64(background.g), amount)),
		b: clamp255(lerp(float64(color.b), float64(background.b), amount)),
	}
}

func parseColor(s string) (rgbColor, bool) {
	r, g, b, ok := parseStyleColor(s)
	if !ok {
		return rgbColor{}, false
	}
	return rgbColor{r: r, g: g, b: b}, true
}

func parseStyleColor(s string) (int, int, int, bool) {
	s = strings.TrimSpace(strings.ToLower(s))
	if strings.HasPrefix(s, "#") {
		s = strings.TrimPrefix(s, "#")
		if len(s) == 3 {
			r, errR := strconv.ParseUint(strings.Repeat(string(s[0]), 2), 16, 8)
			g, errG := strconv.ParseUint(strings.Repeat(string(s[1]), 2), 16, 8)
			b, errB := strconv.ParseUint(strings.Repeat(string(s[2]), 2), 16, 8)
			if errR != nil || errG != nil || errB != nil {
				return 0, 0, 0, false
			}
			return int(r), int(g), int(b), true
		}
		if len(s) == 6 {
			rgb, err := strconv.ParseUint(s, 16, 32)
			if err != nil {
				return 0, 0, 0, false
			}
			return int((rgb >> 16) & 0xFF), int((rgb >> 8) & 0xFF), int(rgb & 0xFF), true
		}
		return 0, 0, 0, false
	}

	idx, err := strconv.Atoi(s)
	if err != nil || idx < 0 || idx > 255 {
		return 0, 0, 0, false
	}
	return xterm256ToRGB(idx)
}

func xterm256ToRGB(idx int) (int, int, int, bool) {
	if idx < 0 || idx > 255 {
		return 0, 0, 0, false
	}
	if idx < 16 {
		palette := [16][3]int{
			{0, 0, 0}, {128, 0, 0}, {0, 128, 0}, {128, 128, 0},
			{0, 0, 128}, {128, 0, 128}, {0, 128, 128}, {192, 192, 192},
			{128, 128, 128}, {255, 0, 0}, {0, 255, 0}, {255, 255, 0},
			{0, 0, 255}, {255, 0, 255}, {0, 255, 255}, {255, 255, 255},
		}
		c := palette[idx]
		return c[0], c[1], c[2], true
	}
	if idx <= 231 {
		n := idx - 16
		r := n / 36
		g := (n / 6) % 6
		b := n % 6
		conv := func(v int) int {
			if v == 0 {
				return 0
			}
			return 55 + v*40
		}
		return conv(r), conv(g), conv(b), true
	}
	gray := 8 + (idx-232)*10
	return gray, gray, gray, true
}

func lerp(a, b, t float64) float64 {
	return a + (b-a)*t
}

func clamp01(v float64) float64 {
	if v < 0 {
		return 0
	}
	if v > 1 {
		return 1
	}
	return v
}

func clamp255(v float64) int {
	if v < 0 {
		return 0
	}
	if v > 255 {
		return 255
	}
	return int(v + 0.5)
}
