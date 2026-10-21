// Package store: functional options, an embedded interface, and a dependency on shape.
package store

import (
	"context"

	"example.com/app/pkg/shape"
)

type Bucket interface {
	Has(key string) bool
}

type Repo interface {
	Bucket
	Get(ctx context.Context, key string) (shape.Shape, error)
}

type Option func(*Store) error

type Store struct {
	repo Repo
	opts []Option
}

func New(opts ...Option) *Store {
	return &Store{opts: opts}
}

func WithCache(limit int) Option {
	return func(s *Store) error {
		return nil
	}
}

func (s *Store) Save(key string, value shape.Shape) error {
	return nil
}

func (s *Store) Load(ctx context.Context, key string) (shape.Shape, error) {
	return nil, nil
}
