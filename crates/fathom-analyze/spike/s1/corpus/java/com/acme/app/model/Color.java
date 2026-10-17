package com.acme.app.model;

public enum Color implements Shape {
    RED(1.0),
    BLUE(0.0);

    private final double weight;

    Color(double weight) {
        this.weight = weight;
    }

    @Override
    public double area() {
        return weight;
    }

    @Override
    public String name() {
        return toString().toLowerCase();
    }

    public static Color byWeight(double w) {
        return w > 0.5 ? RED : BLUE;
    }
}
