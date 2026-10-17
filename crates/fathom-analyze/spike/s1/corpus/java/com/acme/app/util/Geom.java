package com.acme.app.util;

import com.acme.app.model.Shape;

public final class Geom {
    private Geom() {}

    public static double area(Shape s) {
        return s.area();
    }

    public static double perimeter(Shape s) {
        return area(s) * 2;
    }
}

final class Helper {
    int depth() {
        return 0;
    }
}
