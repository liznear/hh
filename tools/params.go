package tools

import (
	"fmt"
	"math"
	"strconv"
)

func requiredString(params map[string]any, key string) (string, error) {
	v, ok := params[key]
	if !ok {
		return "", fmt.Errorf("%s is required", key)
	}
	s, ok := v.(string)
	if !ok || s == "" {
		return "", fmt.Errorf("%s must be a non-empty string", key)
	}
	return s, nil
}

func optionalString(params map[string]any, key string) (string, error) {
	v, ok := params[key]
	if !ok || v == nil {
		return "", nil
	}
	s, ok := v.(string)
	if !ok {
		return "", fmt.Errorf("%s must be a string", key)
	}
	return s, nil
}

func requiredInt(params map[string]any, key string) (int, error) {
	v, ok := params[key]
	if !ok {
		return 0, fmt.Errorf("%s is required", key)
	}
	i, ok := toInt(v)
	if !ok {
		return 0, fmt.Errorf("%s must be an integer", key)
	}
	return i, nil
}

func toInt(v any) (int, bool) {
	switch n := v.(type) {
	case int:
		return n, true
	case int64:
		return int(n), true
	case float64:
		if math.Trunc(n) != n {
			return 0, false
		}
		return int(n), true
	case string:
		i, err := strconv.Atoi(n)
		if err != nil {
			return 0, false
		}
		return i, true
	default:
		return 0, false
	}
}

func optionalInt(params map[string]any, key string, defaultValue int) (int, error) {
	v, ok := params[key]
	if !ok || v == nil {
		return defaultValue, nil
	}
	i, ok := toInt(v)
	if !ok {
		return 0, fmt.Errorf("%s must be an integer", key)
	}
	return i, nil
}

func optionalBool(params map[string]any, key string) (bool, error) {
	v, ok := params[key]
	if !ok || v == nil {
		return false, nil
	}
	b, ok := v.(bool)
	if !ok {
		return false, fmt.Errorf("%s must be a boolean", key)
	}
	return b, nil
}
