package tui

import (
	"encoding/json"
	"fmt"
	"reflect"
	"strconv"
	"strings"

	glamouransi "github.com/charmbracelet/glamour/ansi"
)

// mutedStyleConfig returns a copy of style with muted foreground/background colors.
// amount controls muting strength in [0,1].
func mutedStyleConfig(style glamouransi.StyleConfig, amount float64) glamouransi.StyleConfig {
	amount = clamp01(amount)
	if amount == 0 {
		return style
	}

	out := deepCopyStyleConfig(style)
	muteStyleConfigColors(reflect.ValueOf(&out).Elem(), amount)
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

func muteStyleConfigColors(v reflect.Value, amount float64) {
	if !v.IsValid() {
		return
	}

	t := v.Type()
	switch v.Kind() {
	case reflect.Struct:
		for i := 0; i < v.NumField(); i++ {
			field := v.Field(i)
			fieldType := t.Field(i)
			if !field.CanSet() {
				continue
			}

			if (fieldType.Name == "Color" || fieldType.Name == "BackgroundColor") && field.Kind() == reflect.Ptr && field.Type().Elem().Kind() == reflect.String {
				if field.IsNil() {
					continue
				}
				orig := field.Elem().String()
				muted, ok := muteColorString(orig, amount)
				if !ok {
					continue
				}
				copy := muted
				field.Set(reflect.ValueOf(&copy))
				continue
			}

			muteStyleConfigColors(field, amount)
		}
	case reflect.Ptr:
		if v.IsNil() {
			return
		}
		muteStyleConfigColors(v.Elem(), amount)
	}
}

func muteColorString(s string, amount float64) (string, bool) {
	r, g, b, ok := parseStyleColor(s)
	if !ok {
		return "", false
	}
	r, g, b = muteRGB(r, g, b, amount)
	return fmt.Sprintf("#%02x%02x%02x", r, g, b), true
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

func muteRGB(r, g, b int, amount float64) (int, int, int) {
	gray := 0.299*float64(r) + 0.587*float64(g) + 0.114*float64(b)

	rf := lerp(float64(r), gray, amount)
	gf := lerp(float64(g), gray, amount)
	bf := lerp(float64(b), gray, amount)

	lighten := 0.18 * amount
	rf = lerp(rf, 255, lighten)
	gf = lerp(gf, 255, lighten)
	bf = lerp(bf, 255, lighten)

	return clamp255(rf), clamp255(gf), clamp255(bf)
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
