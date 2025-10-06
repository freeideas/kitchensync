package jLib;
import java.util.*;


public class Pair<A,B> implements Map.Entry<A,B> {
    public final A a;
    public final B b;
    public Pair( A a, B b ) {
        this.a = a;
        this.b = b;
    }
    public int hashCode() {
        return a.hashCode() ^ b.hashCode();
    }
    public boolean equals( Object o ) {
        if ( o == null ) return false;
        if ( o == this ) return true;
        if ( o.getClass() != getClass() ) return false;
        Pair<?,?> p = (Pair<?,?>)o;
        return a.equals(p.a) && b.equals(p.b);
    }
    public String toString() {
        return "(" + a + "," + b + ")";
    }
    @Override
    public A getKey() { return a; }
    @Override
    public B getValue() { return b; }
    @Override
    public B setValue(B value) {
        throw new UnsupportedOperationException("immutable");
    }


    @SuppressWarnings("unused")
    private static boolean pair_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        Pair<String,String> p = new Pair<>("a","b");
        return p.a.equals("a") && p.b.equals("b");
    }


    public static void main( String[] args ) { LibTest.testClass(); }
}