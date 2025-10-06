package jLib;


public class Trio<A,B,C> {
    public final A a;
    public final B b;
    public final C c;
    public Trio( A a, B b, C c ) {
        this.a = a;
        this.b = b;
        this.c = c;
    }
    public int hashCode() {
        return a.hashCode() ^ b.hashCode() ^ c.hashCode();
    }
    public boolean equals( Object o ) {
        if ( o == null ) return false;
        if ( o == this ) return true;
        if ( o.getClass() != getClass() ) return false;
        Trio<?,?,?> p = (Trio<?,?,?>)o;
        return a.equals(p.a) && b.equals(p.b) && c.equals(p.c);
    }
    public String toString() {
        return "(" + a + "," + b + "," + c + ")";
    }


    @SuppressWarnings("unused")
    private static boolean trio_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        Trio<String,String,String> p = new Trio<String,String,String>("a","b","c");
        return p.a.equals("a") && p.b.equals("b") && p.c.equals("c");
    }


    public static void main( String[] args ) { LibTest.testClass(); }
}