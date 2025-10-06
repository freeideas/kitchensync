package jLib;
import java.io.*;
import java.util.*;
import java.math.*;

public class JsonDecoder {

    public static Object decode( Object o ) {
        if (o==null) return null;
        if ( o instanceof Reader r ) {
            JsonDecoder decoder = new JsonDecoder(r);
            try {
                return decoder.decode();
            } catch (IOException ioe) {
                throw new RuntimeException(ioe);
            }
        }
        if ( o instanceof File f ) {
            try( InputStream inp = new FileInputStream(f); ) {
                return decode(inp);
            } catch (IOException ioe) {
                throw new RuntimeException(ioe);
            }
        }
        try {
            return decode( toReader(o,null) );
        } catch (IOException ioe) {
            throw new RuntimeException(ioe);
        }
    }

    public static <K,V> Map<K,V> decodeMap( Object o ) {
        @SuppressWarnings({"unchecked","rawtypes"})
        Map<K,V> result = (Map) decode(o);
        return result;
    }

    public static <T> List<T> decodeList( Object o ) {
        @SuppressWarnings({"unchecked","rawtypes"})
        List<T> result = (List) decode(o);
        return result;
    }

    public static String decodeString( Object o ) {
        String result = (String) decode(o);
        return result;
    }

    public static Number decodeNumber( Object o ) {
        Number result = (Number) decode(o);
        return result;
    }

    public static Long decodeLong( Object o ) {
        Number n = decodeNumber(o);
        if (n==null) return null;
        return n.longValue();
    }

    public static Integer decodeInteger( Object o ) {
        Number n = decodeNumber(o);
        if (n==null) return null;
        return n.intValue();
    }

    public static Double decodeDouble( Object o ) {
        Number n = decodeNumber(o);
        if (n==null) return null;
        return n.doubleValue();
    }

    public static Boolean decodeBoolean( Object o ) {
        o = decode(o);
        if (! (o instanceof Boolean b) ) return null;
        return b;
    }

    public static Reader toReader( Object o, String charsetName ) throws IOException {
        if (charsetName==null) charsetName = "UTF-8";
        if ( o instanceof File ) throw new UnsupportedOperationException("refusing to create a FileInputStream that won't be closed");
        if ( o instanceof Reader r ) return r;
        if ( o instanceof CharSequence cs ) return new StringReader( cs.toString() );
        if ( o instanceof InputStream inp ) return new InputStreamReader( inp, charsetName );
        if ( o instanceof byte[] bytes ) return toReader( new ByteArrayInputStream(bytes), charsetName );
        return toReader( Lib.nvl(o,"").toString(), charsetName );
    }

    // Every "decode" method will return null if it consumes nothing, and return NULL if it consumes "null".
    // This allows the return types of the decode methods to be the same as the types they decode.

    public JsonDecoder(Reader r) {
        this.reader = r;
    }

    public Object decode() throws IOException {
        Object o = rawDecode();
        if (o==NULL) o = null;
        return o;
    }

    private Object rawDecode() throws IOException {
        Object o = null;
        decodeWhitespace();
        if (o==null) o = decodeString();
        if (o==null) o = decodeNumber();
        if (o==null) o = decodeBoolean();
        if (o==null) o = decodeNull();
        if (o==null) o = decodeList();
        if (o==null) o = decodeMap();
        if (o==null) o = decodeIdentifier(); // invalid but allowed
        if (o==null) {
            // at this point the json is invalid,
            // so eat one character and hope the rest is valid
            int c = read();
            if (c<0) return null;
            System.err.println( "ignored invalid character: "+(char)c );
            o = rawDecode();
        }
        return o;
    }

    public <K,V> Map<K,V> decodeMap() throws IOException {
        decodeWhitespace();
        if (expect('{') < 0) return null;
        Map<K, V> map = new LinkedHashMap<>();
        while (true) {
            decodeWhitespace();
            if (expect('}')>=0 || peek()<0 ) break;
            Object keyObj = rawDecode();
            if (keyObj == null) break;
            if (keyObj == NULL) keyObj = null; // not valid JSON, but ok
            @SuppressWarnings("unchecked")
            K key = (K) keyObj;
            decodeWhitespace();
            expect(':');
            decodeWhitespace();
            Object valueObj = rawDecode();
            if (valueObj == null) break;
            if (valueObj == NULL) valueObj = null;
            @SuppressWarnings("unchecked")
            V value = (V) valueObj;
            map.put(key, value);
            decodeWhitespace();
            expect(',');
        }
        return Collections.unmodifiableMap(map);
    }

    @SuppressWarnings("unchecked")
    public <T> List<T> decodeList() throws IOException {
        decodeWhitespace();
        if (expect('[') < 0) return null;
        List<T> list = new ArrayList<>();
        while (true) {
            decodeWhitespace();
            if ( expect(']')>=0 || peek()<0 ) break;
            Object o = rawDecode();
            if (o == NULL) {
                list.add(null);
                o = null;
            }
            T element = (T) o;
            if (o!=null) list.add(element);
            decodeWhitespace();
            expect(',');
        }
        return Collections.unmodifiableList(list);
    }

    public String decodeString() throws IOException {
        decodeWhitespace();
        int quot = expect( '"','\'','`');
        if (quot<0) return null;
        StringBuilder sb = new StringBuilder();
        while (true) {
            int c = read();
            if (c<0) return sb.toString();
            if (c==quot) return sb.toString();
            if (c!='\\') {
                sb.append((char)c);
                continue;
            }
            c = read();
            if (c<0) {
                sb.append('\\');
                return sb.toString();
            }
            switch (c) {
                case '0': sb.append('\0'); break;
                case 'b': sb.append('\b'); break;
                case 'f': sb.append('\f'); break;
                case 'n': sb.append('\n'); break;
                case 'r': sb.append('\r'); break;
                case 't': sb.append('\t'); break;
                case 'u': case 'x': {
                    String prefix = "\\"+(char)c;
                    unread(prefix);
                    int u = decodeUnicodeChar();
                    if (u<0) {
                        sb.append( expect(prefix) );
                    } else {
                        sb.append((char)u);
                    }
                } break;
                default: {
                    sb.append((char)c);
                    break;
                }
            }
        }
    }

    public Long decodeLong() throws IOException {
        return (Long) decodeNumber(true);
    }
    public Double decodeDouble() throws IOException {
        return (Double) decodeNumber(false);
    }
    public Number decodeNumber() throws IOException {
        return decodeNumber(false);
    }
    private Number decodeNumber( boolean forceLong ) throws IOException {
        decodeWhitespace();
        String sign = null;
        sign = sign==null ? expect("-") : sign;
        sign = sign==null ? expect("+") : sign;
        sign = sign==null ? "" : sign;
        if ( expect("NaN") != null ) return Double.NaN;
        if ( expect("Infinity") != null ) return sign.equals("-")
            ? Double.NEGATIVE_INFINITY
            : Double.POSITIVE_INFINITY
        ;
        String prefix = "";
        if ( expect('0') >= 0 ) prefix += "0";
        if ( expect('b') >= 0 ) prefix += "b";
        if ( expect('o') >= 0 ) prefix += "o";
        if ( expect('x') >= 0 ) prefix += "x";
        if ( expect('B') >= 0 ) prefix += "B";
        if ( expect('O') >= 0 ) prefix += "O";
        if ( expect('X') >= 0 ) prefix += "X";
        int radix; String digits;
        final String HEXDIGITS = "0123456789ABCDEF";
        final String OCTDIGITS = "01234567";
        final String BINDIGITS = "01";
        final String DECDIGITS = forceLong ? "1234567_890" : "123_456.789+-eE0";
        int c = prefix.isEmpty() ? -1 : prefix.toUpperCase().charAt( prefix.length() - 1 );
        switch (c) {
            case '0': {
                c = peek();
                if ( c>='0' && c<='9' ) {
                    digits = OCTDIGITS;
                    radix = 8;
                } else {
                    digits = DECDIGITS;
                    radix = 10;
                    unread(prefix);
                }
            } break;
            case 'O': digits=OCTDIGITS; radix=8; break;
            case 'B': digits=BINDIGITS; radix=2; break;
            case 'X': digits=HEXDIGITS; radix=16; break;
            default : digits=DECDIGITS; radix=10; break;
        }
        StringBuilder sb = new StringBuilder();
        boolean isLong = true;
        while ( (c=peek())>=0 && digits.indexOf(c)>=0 ) {
            isLong = isLong && ( c!='.' && c!='e' && c!='E' );
            sb.append( (char)read() );
        }
        while (! sb.isEmpty() ) { // keep backing up until parsing works
            String tryParse = sign+sb;
            tryParse = tryParse.replace("_","");
            try {
                Number n = null;
                if (isLong) {
                    try{ if(n==null) n = Integer.parseInt(tryParse,radix); }catch(Exception willRetry){}
                    try{ if(n==null) n = Long.parseLong(tryParse,radix); }catch(Exception willRetry){}
                    try{ if(n==null && radix==10) n = Double.parseDouble(tryParse); }catch(Exception willRetry){}
                    try{ if(n==null) n = new BigInteger(tryParse,radix); }catch(Exception willRetry){}
                    if (n==null) throw new NumberFormatException();
                } else {
                    try{ if(n==null) n = Double.parseDouble(tryParse); }catch(Exception willRetry){}
                    try{ if(n==null) n = new BigDecimal(tryParse); }catch(Exception willRetry){}
                    if (n==null) throw new NumberFormatException();
                }
                if (forceLong) n = n.longValue();
                return n;
            } catch ( NumberFormatException e ) {
                int newLen = sb.length() - 1;
                if (newLen>=0) {
                    unread( sb.charAt(newLen) );
                    sb.setLength(newLen);
                }
            }
        }
        // failed to parse number
        unread(sign+prefix);
        return null;
    }

    public Boolean decodeBoolean() throws IOException {
        decodeWhitespace();
        String s = expect("true","false");
        if (s==null) return null;
        return Boolean.valueOf(s);
    }

    public Object decodeNull() throws IOException {
        decodeWhitespace();
        if ( expect("null") == null ) return null;
        return NULL;
    }

    public int decodeUnicodeChar() throws IOException {
        decodeWhitespace();
        String prefix = expect( "\\u{", "\\u", "\\x" );
        if (prefix==null) return -1;
        StringBuilder sb = new StringBuilder();
        sb.append(prefix);
        try {
            int digits = prefix.indexOf('u')>=0 ? 4 : 2;
            boolean inCurly = prefix.endsWith("{");
            for (int i=0; i<digits; i++) {
                int c = read();
                if (c<0) return -1;
                sb.append((char)c);
                if ( (c<'0'&&c>'9') && (c<'a'&&c>'f') && (c<'A'&&c>'F') ) return -1;
            }
            if (inCurly) {
                if ( expect('}') < 0 ) return -1;
                sb.append('}');
            }
            String hexDigits = sb.substring( prefix.length(), prefix.length()+digits );
            try {
                int u = Integer.parseInt(hexDigits,16);
                sb = null;
                return u;
            } catch ( NumberFormatException e ) { return -1; }
        } finally {
            if (sb!=null) unread(sb);
        }
    }

    public String decodeWhitespace() throws IOException {
        // whitespace might include a comment
        StringBuilder sb = new StringBuilder();
        while (true) {
            int c = expect(' ','\t','\r','\n','/');
            if (c<0) return sb.isEmpty() ? null : sb.toString();
            if (c=='/') {
                unread(c);
                String comment = decodeComment();
                if (comment!=null) sb.append(comment);
            } else {
                sb.append((char)c);
            }
        }
    }

    public String decodeComment() throws IOException {
        if ( peek() != '/' ) return null;
        int c = read(); // consume the '/' permanently, even if it's not a comment
        c = read();
        String ending ;
        switch ( c ) {
            case '/': ending = "\n"; break;
            case '*': ending = "*/"; break;
            default: unread(c); return null;
        }
        StringBuilder sb = new StringBuilder();
        sb.append( '/' );
        sb.append( (char)c );
        while ( true ) {
            c = read();
            if (c<0) return sb.toString();
            sb.append( (char)c );
            if ( sb.toString().endsWith(ending) ) break;
        }
        return sb.toString();
    }

    public String decodeIdentifier() throws IOException {
        int c = peek();
        if (! Character.isAlphabetic(c) ) return null;
        StringBuilder sb = new StringBuilder();
        while ( Character.isAlphabetic(c) || Character.isDigit(c) ) {
            sb.append( (char)read() );
            c = peek();
        }
        return sb.toString();
    }

    private final Reader reader;
    private final StringBuilder unreadBuffer = new StringBuilder();
    public static final Object NULL = new Object() {
        public String toString() { return "null"; }
    };

    private int expect( char ... chars ) throws IOException {
        int c = read();
        for (char ch : chars) {
            if (c==ch) return c;
        }
        unread(c);
        return -1;
    }

    private String expect( String ... strings ) throws IOException {
        // returns the first longest string that matches
        int maxLen=0;
        for ( String s : strings ) maxLen = Math.max( maxLen, s==null?0:s.length() );
        StringBuilder sb = new StringBuilder();
        try {
            for ( int i=0; i<maxLen; i++ ) {
                int c = read();
                if (c<0) break;
                sb.append((char)c);
            }
            String tryMatch = sb.toString();
            String longestMatch = "";
            int longestLen = 0;
            for ( String s : strings ) {
                if (s==null) continue;
                if ( tryMatch.startsWith(s) && s.length()>longestLen ) {
                    longestMatch = s;
                    longestLen = s.length();
                }
            }
            sb.delete(0,longestLen);
            return longestLen==0 ? null : longestMatch;
        } finally {
            if(sb!=null) unread(sb);
        }
    }

    private int read() throws IOException {
        if (! unreadBuffer.isEmpty() ) {
            int idx = unreadBuffer.length()-1;
            int c = unreadBuffer.charAt(idx);
            unreadBuffer.setLength(idx);
            return c;
        }
        int c = reader.read();
        //System.out.print( (char)c );
        /*
        if (c>=0) {
            System.out.print( (char)c );
        } else {
            System.out.print( "<EOF>" );
        }
        */
        return c;
    }

    private int peek() throws IOException {
        int c = read();
        unread(c);
        return c;
    }

    private void unread( int c ) {
        if (c>=0) unreadBuffer.append((char)c);
    }
    private void unread( CharSequence s ) {
        if (s==null) return;
        for ( int i=s.length()-1; i>=0; i-- ) unread( s.charAt(i) );
    }

    @SuppressWarnings("unused")
    private static boolean test_TEST_() throws Exception {
        try { // test decodeNull()
            StringReader sr = new StringReader("null");
            LibTest.asrtEQ( new JsonDecoder(sr).decodeNull(), NULL );
            sr = new StringReader( "nulX" );
            LibTest.asrtEQ( new JsonDecoder(sr).decodeNull(), null );
        } catch (IOException ioe) { throw new RuntimeException(ioe); }
        { // test decodeBoolean()
            String input = "true";
            LibTest.asrtEQ(JsonDecoder.decodeBoolean(input), Boolean.TRUE);
            input = "false";
            LibTest.asrtEQ(JsonDecoder.decodeBoolean(input), Boolean.FALSE);
            input = "truX";
            LibTest.asrtEQ(JsonDecoder.decodeBoolean(input), null);
        }
        { // test decodeString()
            String input, output, expected;
            input = "'He said,\\t\"hello\"'";
            expected = "He said,\t\"hello\"";
            output = JsonDecoder.decodeString(input);
            LibTest.asrtEQ(output, expected);
            input = "\\u0041\\u{0042}\\x43\\uBAD\\u{BAD}\\u{BADBAD}\\x{BA}\\xB\\u00F";
            expected = "ABC" + input.substring(input.indexOf("\\uBAD"));
            //output = JsonDecoder.decodeString(input);
            input = "`backquoted 'string'`";
            expected = "backquoted 'string'";
            output = JsonDecoder.decodeString(input);
            LibTest.asrtEQ(output, expected);
            input = "'\\uD83D\\uDE3A'(unicode cat)";
            expected = "\uD83D\uDE3A";
            output = JsonDecoder.decodeString(input);
            LibTest.asrtEQ(output, expected);
            input = "\"\\uD83D\\uDC36\"(unicode dog)";
            expected = "\uD83D\uDC36";
            output = JsonDecoder.decodeString(input);
            LibTest.asrtEQ(output, expected);
        }
        { // test decodeNumber()
            String input;
            Number output;
            Number expected;
            input = "-1234.5678e-90";
            expected = Double.parseDouble(input);
            output = JsonDecoder.decodeNumber(input);
            LibTest.asrtEQ(output, expected);
            input += "e10"; // should be ignored
            output = JsonDecoder.decodeNumber(input);
            LibTest.asrtEQ(output, expected);
            input = "5" + input;
            expected = (double) 5;
            output = JsonDecoder.decodeNumber(input);
            LibTest.asrtEQ(output, expected);
            input = "0x1234";
            expected = (double) 0x1234;
            output = JsonDecoder.decodeNumber(input);
            LibTest.asrtEQ(output, expected);
            input = "0x1234.5678";
            expected = (double) 0x1234;
            output = JsonDecoder.decodeNumber(input);
            LibTest.asrtEQ(output, expected);
            input = "0b1010";
            expected = (double) 0b1010;
            output = JsonDecoder.decodeNumber(input);
            LibTest.asrtEQ(output, expected);
            input = "0o12349";
            expected = (double) 01234;
            output = JsonDecoder.decodeNumber(input);
            LibTest.asrtEQ(output, expected);
            input = "0778";
            expected = (double) 077;
            output = JsonDecoder.decodeNumber(input);
            LibTest.asrtEQ(output, expected);
            input = "-1_234.5678";
            expected = -1_234L;
            output = JsonDecoder.decodeLong(input);
            LibTest.asrtEQ(output, expected);
            LibTest.asrtEQ(Long.class, expected.getClass());
            // test for NaN and Infinity
            input = "-NaN";
            expected = Double.NaN;
            output = JsonDecoder.decodeNumber(input);
            LibTest.asrtEQ(output, expected);
            input = "-Infinity";
            expected = Double.NEGATIVE_INFINITY;
            output = JsonDecoder.decodeNumber(input);
            LibTest.asrtEQ(output, expected);
        }
        { // test decodeList()
            String input;
            Object output;
            Object expected;
            input = "['ok',true,1234.5678]";
            expected = List.of("ok", true, 1234.5678);
            output = JsonDecoder.decodeList(input);
            LibTest.asrtEQ(output, expected);
        }
        { // test decodeMap()
            String input;
            Object output;
            Object expected;
            input = "{'ok':true,'num':1234.567,'list':[1,2,3],\"key\":null}";
            expected = Map.of(
                "ok", true,
                "num", 1234.567,
                "list", List.of(1, 2, 3)
            );
            @SuppressWarnings({"unchecked","rawtypes"})
            Map<String,Object> m = (Map) expected;
            m = new LinkedHashMap<>(m);
            m.put("key",null);
            expected = m;
            output = JsonDecoder.decodeMap(input);
            LibTest.asrtEQ(output, expected);
            // duplicate keys
            input = "{'ok':true,'ok':`false`}";
            expected = Map.of("ok",false);
            output = JsonDecoder.decodeMap(input);
            LibTest.asrtEQ(output, expected);
        }
        { // test various tricky stuff
            String input;
            Object output,expected;
            input = """
                /* \n comment \n */ { // comment
                    "str1" /* */ : /* */  "Line1\\nLine2",
                    "str2"/**/:/**/"This is a tab: \\t.", // comment
                    "str3": // comment /* tricky
                    "Quote: \\".",
                    "str4": "Unicode snowman: \\u2603",
                    "obj1": { // comment
                        "key1": "Value with a backslash: \\\\",
                        "not a comment" : "/* not a comment */",
                        "not a comment" : "// not a comment",
                        "key2": "Unicode cat: \\uD83D\\uDE3A",
                        "arr": [
                            "Unicode dog: \\uD83D\\uDC36",
                            -Infinity, -NaN
                        ],
                    }
                /*comment*/}//comment
                "this should not be parsed"
            """;
            expected = Map.of(
                "str1", "Line1\nLine2",
                "str2", "This is a tab: \t.",
                "str3", "Quote: \".",
                "str4", "Unicode snowman: \u2603",
                "obj1", Map.of(
                    "key1", "Value with a backslash: \\",
                    "not a comment", "// not a comment",
                    "key2", "Unicode cat: \uD83D\uDE3A",
                    "arr", List.of(
                        "Unicode dog: \uD83D\uDC36",
                        Double.NEGATIVE_INFINITY, -Double.NaN
                    )
                )
            );
            output = JsonDecoder.decode(input);
            LibTest.asrtEQ(output,expected);
            input =
            """
                { "index": 0, "embedding": [ 0.021137446, 0.0025192886, -0.003262786, -0.033500392 ] }
            """;
            output = JsonDecoder.decode(input);
            expected = Map.of(
                "index", 0,
                "embedding", List.of(
                    0.021137446, 0.0025192886, -0.003262786, -0.033500392
                )
            );
            LibTest.asrtEQ(output,expected);
            input = "[ {}, [] ]";
            output = JsonDecoder.decode(input);
            expected = List.of( Map.of(), List.of() );
            LibTest.asrtEQ(output,expected);
            input = """
                [ { "group": null, "is_blocking": false } ]
            """;
            output = JsonEncoder.encode( JsonDecoder.decode(input) );
            expected = input.replaceAll( "\\s+", "" );
            LibTest.asrtEQ(output,expected);
        }
        if ( System.currentTimeMillis() < 0 ) { // forgive invalid JSON
            String input;
            Object output,expected;
            input = "{'ok':true,'ok':`false`";
            output = JsonDecoder.decode(input);
            expected = JsonDecoder.decode( "{'ok':true,'ok':`false`}" );
            LibTest.asrtEQ(output,expected);
            input = """
                ;{,'ok':true,,,'ok':`false`,null,};
            """;
            output = JsonDecoder.decode(input);
            LibTest.asrtEQ(output,expected);
            input = "{ok:true,one:two}";
            expected = Map.of( "ok",true, "one","two" );
            output = JsonDecoder.decode(input);
            LibTest.asrtEQ(output,expected);
        }
        return true;
    }

    public static void main(String[] args) throws Exception { LibTest.testClass(); }
}
