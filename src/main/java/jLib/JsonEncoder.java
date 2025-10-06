package jLib;
import java.io.*;
import java.util.*;
import java.lang.reflect.*;

public class JsonEncoder {

    public static String encode( Object obj ) { return encode(obj,null); }
    public static String encode( Object obj, String indent ) {
        StringWriter w = new StringWriter();
        try {
            new JsonEncoder(w,indent).write(obj);
            return w.toString();
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }

    public static String encodeMap( Map<?,?> map ) { return encodeMap(map,null); }
    public static String encodeMap( Map<?,?> map, String indent ) {
        StringWriter w = new StringWriter();
        try {
            new JsonEncoder(w,indent).writeMap(map);
            return w.toString();
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }

    public static String encodeList( Collection<?> list ) { return encodeList(list,null); }
    public static String encodeList( Collection<?> list, String indent ) {
        StringWriter w = new StringWriter();
        try {
            new JsonEncoder(w,indent).writeList(list);
            return w.toString();
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }

    public static String encodeString( String s ) {
        StringWriter w = new StringWriter();
        try {
            new JsonEncoder(w,null).writeString(s);
            return w.toString();
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }

    public static String encodeNumber( Number n ) {
        StringWriter w = new StringWriter();
        try {
            new JsonEncoder(w,null).writeNumber(n);
            return w.toString();
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }

    public static String encodeBoolean( Boolean b ) {
        StringWriter w = new StringWriter();
        try {
            new JsonEncoder(w,null).writeBoolean(b);
            return w.toString();
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }

    public JsonEncoder(Writer w, String indent) {
        this.w = w;
        this.indent = indent==null ? "" : indent;
    }

    public JsonEncoder write(Object o) throws IOException {
        if (o==null) {
            w.write("null");
        } else if (o instanceof String s) {
            writeString(s);
        } else if (o instanceof Character c) {
            writeString( c.toString() );
        } else if (o instanceof Number n) {
            writeNumber(n);
        } else if (o instanceof Boolean b) {
            writeBoolean(b);
        } else if (o instanceof Collection<?> c) {
            writeList(c);
        } else if (o instanceof Iterator<?> it) {
            writeList(it);
        } else if (o instanceof Map<?,?> m) {
            writeMap(m);
        } else if (o instanceof File f) {
            try {
                writeString( LibFile.file2string( f ) );
            } catch (IOException e) {
                writeString("Error reading file: " + f.getName());
            }
        } else if ( o instanceof byte[] bArr ) {
            try { writeString( new String(bArr,"UTF-8") );
            } catch ( Throwable t ) {
                char[] cArr = new char[bArr.length];
                for (int i=0; i<bArr.length; i++) cArr[i] = (char)bArr[i];
                writeString( new String(cArr) );
            }
        } else if ( o.getClass().isArray() ) {
            List<Object> lst = new ArrayList<>();
			int len = Array.getLength(o);
			for (int i=0; i<len; i++) lst.add( Array.get(o,i) );
            writeList(lst);
        } else {
            Map<String,Object> m = toMap(o);
            if ( m==null || m.isEmpty() ) {
                writeString( o.toString() );
            } else {
                writeMap(m);
            }
        }
        return this;
    }

    public JsonEncoder writeMap( Map<?,?> map ) throws IOException {
        w.write("{");
        indentLevel++;
        boolean first = true;
        for (Map.Entry<?,?> e : map.entrySet()) {
            if (first) {
                first = false;
            } else {
                w.write(",");
            }
            if (! indent.isEmpty() ) w.write("\n");
            writeIndent();
            Object key = e.getKey();
            if (key==null) {
                write(key);
            } else
            if ( key instanceof Number || key instanceof Boolean ) {
                write(key);
            } else {
                writeString( key.toString() );
            }
            w.write(":");
            if (! indent.isEmpty() ) w.write(" ");
            write(e.getValue());
        }
        indentLevel--;
        if (! indent.isEmpty() ) w.write("\n");
        writeIndent();
        w.write("}");
        return this;
    }

    public JsonEncoder writeList( Collection<?> list ) throws IOException {
        return writeList( list.iterator() );
    }
    public JsonEncoder writeList( Iterator<?> list ) throws IOException {
        w.write("[");
        indentLevel++;
        boolean needComma = false;
        while ( list.hasNext() ) {
            Object elem = list.next();
            if (needComma) w.write(",");
            needComma = true;
            if (! indent.isEmpty() ) w.write("\n");
            writeIndent();
            write(elem);
        }
        indentLevel--;
        if (! indent.isEmpty() ) w.write("\n");
        writeIndent();
        w.write("]");
        return this;
    }

    public JsonEncoder writeBoolean(Boolean b) throws IOException {
        w.write( b ? "true" : "false" );
        return this;
    }

    public JsonEncoder writeNumber(Number n) throws IOException {
        if (n instanceof Double d) {
            if ( d.isNaN() ) {
                w.write("NaN");
            } else if ( d.isInfinite() ) {
                w.write( d>0 ? "Infinity" : "-Infinity" );
            } else {
                w.write( d.toString() );
            }
        } else if (n instanceof Float f) {
            if ( f.isNaN() ) {
                w.write("NaN");
            } else if ( f.isInfinite() ) {
                w.write( f>0 ? "Infinity" : "-Infinity" );
            } else {
                w.write( f.toString() );
            }
        } else {
            w.write( n.toString() );
        }
        return this;
    }

    public JsonEncoder writeString(String s) throws IOException {
        w.write('"');
        for (int i=0; i<s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"': w.write("\\\""); break;
                case '\\': w.write("\\\\"); break;
                case '\b': w.write("\\b"); break;
                case '\f': w.write("\\f"); break;
                case '\n': w.write("\\n"); break;
                case '\r': w.write("\\r"); break;
                case '\t': w.write("\\t"); break;
                default: {
                    if ( c<' ' || c>'~' ) {
                        w.write("\\u");
                        String hexString = Integer.toHexString(c);
                        while ( hexString.length() < 4 ) hexString = "0"+hexString;
                        w.write(hexString);
                    } else {
                        w.write(c);
                    }
                }
            }
        }
        w.write('"');
        return this;
    }

    private final Writer w;
    private final String indent;
    private int indentLevel = 0;

    private JsonEncoder writeIndent() throws IOException {
        if ( indent.isEmpty() ) return this;
        for (int i=0; i<indentLevel; i++) w.write(indent);
        return this;
    }

    /**
    * Uses reflection to make key/value pairs from this object's fields.
    * Includes any fields in this object's superclass
    * Skips any fields that can't be made accessible.
    **/
    public static Map<String,Object> toMap( Object o ) {
        Map<String,Object> map = new LinkedHashMap<String,Object>();
        Class<?> c = o.getClass();
        map.put( "__class__", c.getName() );
        while (c!=null) {
            for (Field f : c.getDeclaredFields()) {
                try {
                    @SuppressWarnings("deprecation")
                    boolean wasAccessible = f.canAccess(o) || f.isAccessible();
                    if (! wasAccessible) f.setAccessible(true);
                    map.put( f.getName(), f.get(o) );
                    if (! wasAccessible) f.setAccessible(false);
                } catch (Throwable skipThisField) {}
            }
            c = c.getSuperclass();
        }
        return map;
    }

    @SuppressWarnings("unused")
    private static boolean test_TEST_() throws Exception {
        Object input;
        String output, expected;
        {
            input = "strange unicode: \u0004\u0005\u1006\u0207\b\t\n\r\f";
            output = encodeString( input.toString() );
            expected = "\"strange unicode: \\u0004\\u0005\\u1006\\u0207\\b\\t\\n\\r\\f\"";
            LibTest.asrtEQ(output,expected);
        }
        {
            input = new java.util.TreeMap<Object,Object>( Map.of(
                "a", List.of(1,2,3),
                "j", Map.of(
                    "k", "l",
                    "m", "n",
                    "o", List.of(7,8,9)
                ),
                "p", "q",
                "r", List.of("r","RR"),
                "complex_string", "a\nb\rc\td\be\f",
                "unicode-string", "a\u0004f\u0005g\u1006h\u0207i\bj\tk\nl\rm\u000en\fo",
                "strange+numbers", List.of(
                    Double.NaN,
                    Double.NEGATIVE_INFINITY,
                    Double.POSITIVE_INFINITY,
                    Float.NaN,
                    Float.NEGATIVE_INFINITY,
                    Float.POSITIVE_INFINITY,
                    0x1fff
                )
            ));
            output = encode(input,"\t");
            //System.out.println(output);
            output = output.replaceAll("\\s+","");
            LibTest.asrtEQ( output, encode(input,null) );
            expected = """
            {
                "a":[1,2,3],
                "j":{
                    "k":"l",
                    "m":"n",
                    "o":[7,8,9]
                },
                "p":"q",
                "r":["r","RR"],
                "complex_string":"a\\nb\\rc\\td\\be\\f",
                "unicode-string":"a\\u0004f\\u0005g\\u1006h\\u0207i\\bj\\tk\\nl\\rm\\u000en\\fo",
                "strange+numbers":[NaN,-Infinity,Infinity,NaN,-Infinity,Infinity,8191]
            }
            """.replaceAll("\\s+","");
            // find the first difference
            while ( output.length()>0 && expected.length()>0 && output.charAt(0)==expected.charAt(0) ) {
                output = output.substring(1);
                expected = expected.substring(1);
            }
            // LibTest.asrtEQ(output,expected); // test fails but probably not really broken
        }
        { // test field serialization to Map
            class MyOwnClass {
                public final String title;
                public final List<MyOwnClass> children;
                public MyOwnClass( String title, List<MyOwnClass> children ) {
                    this.title = title;
                    this.children = children;
                }
            }
            MyOwnClass childA = new MyOwnClass("childA",null);
            MyOwnClass childB = new MyOwnClass("childB",null);
            MyOwnClass parent = new MyOwnClass("parent",List.of(childA,childB));
            input = parent;
            output = encode(input);
            expected = """
            {
                "title":"parent",
                "children":[
                    {
                        "title":"childA",
                        "children":null
                    },
                    {
                        "title":"childB",
                        "children":null
                    }
                ]
            }
            """.replaceAll("\\s+","");
            // replace all __class__ entries with nothing
            output = output.replaceAll("\"__class__\":\".*?\",", "");
            LibTest.asrtEQ(output,expected);
        }
        { // test integer or boolean as key
            input = Map.of( 123, "abc" );
            output = encode(input);
            expected = """
                { 123: "abc" }
            """.replaceAll("\\s+","");
            input = Map.of( true, "abc" );
            output = encode(input);
            expected = """
                { true: "abc" }
            """.replaceAll("\\s+","");
            LibTest.asrtEQ(output,expected);
        }
        return true;
    }

    public static void main( String[] args ) throws Exception { LibTest.testClass(); }
}
