package com.acme.app.util;

import com.acme.app.model.*;
import java.util.*;

public final class Wildcard {
    private Wildcard() {}

    public static Shape pick(String kind) {
        return Circle.unit();
    }

    public static List<String> kinds() {
        return List.of("circle", "point");
    }
}
