package config

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/provider"
)

const defaultRelPath = ".config/hh/config.json"

type Config struct {
	Models    ModelSelectionConfig      `json:"models"`
	Providers map[string]ProviderConfig `json:"providers"`
}

type ModelSelectionConfig struct {
	Default string `json:"default"`
}

type ProviderConfig struct {
	DisplayName string                 `json:"display_name"`
	BaseURL     string                 `json:"base_url"`
	APIKeyEnv   string                 `json:"api_key_env"`
	Models      map[string]ModelConfig `json:"models"`
}

type ModelConfig struct {
	ID          string            `json:"id"`
	DisplayName string            `json:"display_name"`
	Limits      ModelLimitsConfig `json:"limits"`
}

type ModelLimitsConfig struct {
	Context int `json:"context"`
	Output  int `json:"output"`
}

type ModelRef struct {
	Name            string
	ProviderName    string
	ProviderBaseURL string
	ProviderAPIKey  string
	ModelID         string
	ContextWindow   int
}

func DefaultPath() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("resolve home directory: %w", err)
	}
	return filepath.Join(home, defaultRelPath), nil
}

func Load() (Config, error) {
	path, err := DefaultPath()
	if err != nil {
		return Config{}, err
	}
	return loadFromPath(path)
}

func loadFromPath(path string) (Config, error) {
	buf, err := os.ReadFile(path)
	if err != nil {
		return Config{}, err
	}

	var cfg Config
	if err := json.Unmarshal(buf, &cfg); err != nil {
		return Config{}, fmt.Errorf("parse config %s: %w", path, err)
	}
	defaultModel := cfg.DefaultModel()
	if defaultModel == "" {
		return Config{}, fmt.Errorf("missing models.default in hh config")
	}
	if !cfg.HasModel(defaultModel) {
		return Config{}, fmt.Errorf("model %q is not configured in hh config", defaultModel)
	}
	return cfg, nil
}

func (f Config) ModelRefs() []ModelRef {
	if len(f.Providers) == 0 {
		return nil
	}

	providerNames := make([]string, 0, len(f.Providers))
	for providerName := range f.Providers {
		providerNames = append(providerNames, providerName)
	}
	sort.Strings(providerNames)

	ret := make([]ModelRef, 0)
	for _, providerName := range providerNames {
		providerCfg := f.Providers[providerName]
		if len(providerCfg.Models) == 0 {
			continue
		}

		modelNames := make([]string, 0, len(providerCfg.Models))
		for modelName := range providerCfg.Models {
			modelNames = append(modelNames, modelName)
		}
		sort.Strings(modelNames)

		for _, modelName := range modelNames {
			modelCfg := providerCfg.Models[modelName]
			modelID := strings.TrimSpace(modelCfg.ID)
			if modelID == "" {
				modelID = modelName
			}
			ret = append(ret, ModelRef{
				Name:            providerName + "/" + modelName,
				ProviderName:    providerName,
				ProviderBaseURL: strings.TrimSpace(providerCfg.BaseURL),
				ProviderAPIKey:  strings.TrimSpace(providerCfg.APIKeyEnv),
				ModelID:         modelID,
				ContextWindow:   modelCfg.Limits.Context,
			})
		}
	}
	return ret
}

func (f Config) DefaultModel() string {
	return strings.TrimSpace(f.Models.Default)
}

func (f Config) AvailableModels() []string {
	refs := f.ModelRefs()
	ret := make([]string, 0, len(refs))
	for _, ref := range refs {
		ret = append(ret, ref.Name)
	}
	return ret
}

func (f Config) ModelContextWindows() map[string]int {
	refs := f.ModelRefs()
	ret := make(map[string]int, len(refs))
	for _, ref := range refs {
		if ref.ContextWindow > 0 {
			ret[ref.Name] = ref.ContextWindow
		}
	}
	return ret
}

func (f Config) HasModel(modelName string) bool {
	modelName = strings.TrimSpace(modelName)
	if modelName == "" {
		return false
	}
	for _, name := range f.AvailableModels() {
		if name == modelName {
			return true
		}
	}
	return false
}

func (f Config) ModelRouterProvider() (agent.Provider, error) {
	modelRefs := f.ModelRefs()
	if len(modelRefs) == 0 {
		return nil, fmt.Errorf("no models configured in hh config")
	}

	routes := make(map[string]provider.ModelRoute, len(modelRefs))
	providerCache := map[string]agent.Provider{}
	for _, modelRef := range modelRefs {
		p, ok := providerCache[modelRef.ProviderName]
		if !ok {
			apiKey := ""
			if modelRef.ProviderAPIKey != "" {
				apiKey = os.Getenv(modelRef.ProviderAPIKey)
			}
			p = provider.NewOpenAICompatibleProvider(modelRef.ProviderBaseURL, apiKey)
			providerCache[modelRef.ProviderName] = p
		}

		routes[modelRef.Name] = provider.ModelRoute{
			Provider: p,
			Model:    modelRef.ModelID,
		}
	}

	return provider.NewModelRouterProvider(routes), nil
}
