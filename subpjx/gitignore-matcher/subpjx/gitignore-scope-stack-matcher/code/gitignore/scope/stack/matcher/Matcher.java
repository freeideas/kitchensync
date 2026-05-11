package gitignore.scope.stack.matcher;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

public final class Matcher {
    private final List<Layer> layers;

    private Matcher(List<Layer> layers) {
        this.layers = Collections.unmodifiableList(layers);
    }

    public static Matcher empty() {
        return new Matcher(new ArrayList<>());
    }

    public static Matcher pushScope(Matcher parent, String scopeDir, List<CompiledPattern> patternSet) {
        List<Layer> next = new ArrayList<>(parent.layers);
        next.add(new Layer(scopeDir, List.copyOf(patternSet)));
        return new Matcher(next);
    }

    public int layerCount() {
        return layers.size();
    }

    public Layer layerAt(int index) {
        return layers.get(index);
    }

    public List<Layer> layers() {
        return layers;
    }
}
