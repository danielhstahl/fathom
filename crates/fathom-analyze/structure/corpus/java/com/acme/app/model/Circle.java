package com.acme.app.model;

import com.acme.app.Tracked;

@Tracked("circle")
public record Circle(String name, double r) implements Shape {
    public Circle {
        if (r < 0) {
            throw new IllegalArgumentException("r");
        }
    }

    @Override
    public double area() {
        return Math.PI * r * r;
    }

    public Point center() {
        return new Point(0, 0);
    }

    public static Circle unit() {
        return new Circle("unit", 1);
    }
}
