package agent

type Config struct {
	provider Provider
}

func NewConfig(provider Provider) Config {
	return Config{provider: provider}
}
