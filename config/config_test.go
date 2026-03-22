package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestModelRefsSortedAndQualified(t *testing.T) {
	cfg := Config{
		Providers: map[string]ProviderConfig{
			"z": {
				BaseURL:   "https://z.example",
				APIKeyEnv: "Z_KEY",
				Models: map[string]ModelConfig{
					"m2": {ID: "id-2"},
					"m1": {ID: "id-1"},
				},
			},
			"a": {
				BaseURL:   "https://a.example",
				APIKeyEnv: "A_KEY",
				Models: map[string]ModelConfig{
					"m3": {ID: ""},
				},
			},
		},
	}

	refs := cfg.ModelRefs()
	if len(refs) != 3 {
		t.Fatalf("len(refs) = %d, want 3", len(refs))
	}
	if refs[0].Name != "a/m3" || refs[0].ModelID != "m3" {
		t.Fatalf("refs[0] = %#v, want a/m3->m3", refs[0])
	}
	if refs[1].Name != "z/m1" || refs[1].ModelID != "id-1" {
		t.Fatalf("refs[1] = %#v, want z/m1->id-1", refs[1])
	}
	if refs[2].Name != "z/m2" || refs[2].ModelID != "id-2" {
		t.Fatalf("refs[2] = %#v, want z/m2->id-2", refs[2])
	}
}

func TestAvailableModelsAndContextWindows(t *testing.T) {
	cfg := Config{
		Providers: map[string]ProviderConfig{
			"proxy": {
				Models: map[string]ModelConfig{
					"m2": {ID: "id-2", Limits: ModelLimitsConfig{Context: 2000}},
					"m1": {ID: "id-1", Limits: ModelLimitsConfig{Context: 1000}},
				},
			},
		},
	}

	models := cfg.AvailableModels()
	if len(models) != 2 {
		t.Fatalf("len(models) = %d, want 2", len(models))
	}
	if models[0] != "proxy/m1" || models[1] != "proxy/m2" {
		t.Fatalf("models = %#v, want [proxy/m1 proxy/m2]", models)
	}

	windows := cfg.ModelContextWindows()
	if windows["proxy/m1"] != 1000 {
		t.Fatalf("context proxy/m1 = %d, want 1000", windows["proxy/m1"])
	}
	if windows["proxy/m2"] != 2000 {
		t.Fatalf("context proxy/m2 = %d, want 2000", windows["proxy/m2"])
	}

	if !cfg.HasModel("proxy/m2") {
		t.Fatal("HasModel(proxy/m2) = false, want true")
	}
	if cfg.HasModel("missing/model") {
		t.Fatal("HasModel(missing/model) = true, want false")
	}
}

func TestModelRouterProviderRequiresConfiguredModels(t *testing.T) {
	_, err := (Config{}).ModelRouterProvider()
	if err == nil {
		t.Fatal("ModelRouterProvider() error = nil, want non-nil")
	}
}

func TestLoad_ValidatesDefaultModel(t *testing.T) {
	tests := []struct {
		name        string
		jsonContent string
		wantErrPart string
	}{
		{
			name: "missing default",
			jsonContent: `{
				"models": {},
				"providers": {
					"proxy": {"models": {"glm-5": {"id": "glm-5"}}}
				}
			}`,
			wantErrPart: "missing models.default",
		},
		{
			name: "default not configured",
			jsonContent: `{
				"models": {"default": "proxy/gpt-5"},
				"providers": {
					"proxy": {"models": {"glm-5": {"id": "glm-5"}}}
				}
			}`,
			wantErrPart: "is not configured",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "config.json")
			if err := os.WriteFile(path, []byte(tt.jsonContent), 0o644); err != nil {
				t.Fatalf("WriteFile() error = %v", err)
			}
			_, err := loadFromPath(path)
			if err == nil {
				t.Fatal("loadFromPath() error = nil, want non-nil")
			}
			if !strings.Contains(err.Error(), tt.wantErrPart) {
				t.Fatalf("loadFromPath() error = %q, want containing %q", err.Error(), tt.wantErrPart)
			}
		})
	}
}
