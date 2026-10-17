package com.acme.app.model;

public sealed interface Shape permits Circle, Point, Color {
    double area();

    String name();

    default String describe() {
        return name() + ":" + area();
    }

    static Shape none() {
        return Color.RED;
    }
}
