package common

import "context"

func BridgeChannel[In any, Out any](ctx context.Context, in <-chan In, out chan<- Out, f func(v In) Out) {
	defer close(out)
	for {
		select {
		case v, ok := <-in:
			if !ok {
				return
			}
			select {
			case out <- f(v):
			case <-ctx.Done():
			}
		case <-ctx.Done():
			return
		}
	}
}

func FlattenBridgeChannel[In any, Out any](ctx context.Context, in <-chan In, out chan<- Out, f func(v In) []Out) {
	defer close(out)
	for {
		select {
		case v, ok := <-in:
			if !ok {
				return
			}
			for _, o := range f(v) {
				select {
				case out <- o:
				case <-ctx.Done():
				}
			}
		case <-ctx.Done():
			return
		}
	}
}
