package jLib;
import java.io.*;
import java.net.*;
import java.nio.charset.*;
import java.util.*;
import java.util.regex.*;
import javax.script.*;


public class LibString {

    public static final String[][] XMLENTITIES = {
        { "&lt;",   "<" },
        { "&gt;",   ">" },
        { "&amp;",  "&" },
        { "&apos;", "'" },
        { "&quot;", "\"" },
    };


    public static String xmlSafeText( Object txt ) {
        return xmlSafeText(txt,null);
    }
    public static String xmlSafeText( Object txt, Boolean encodeAll ) {
        if (txt==null) txt="";
        String str = txt.toString();
        int c, len=str.length();
        StringBuffer buf = new StringBuffer( len + len<<1 );
        for (int i=0; i<len; i++) {
            c = str.charAt(i);
            if ( encodeAll==Boolean.FALSE && !( c=='<' || c=='&' ) ) {
                // in this mode, encode only < and &
                buf.append( (char)c );
                continue;
            }
            if ( encodeAll!=Boolean.TRUE && c=='&' ) {
                // in these modes, skip already-encoded entities
                int end = Math.min( i+12, str.length() );
                if ( str.substring(i,end).matches("^&(([a-zA-Z0-9]+)|(#\\d+));.*$") ) {
                    while (c!=';') {
                        buf.append( (char)c );
                        i+=1; c=str.charAt(i);
                    }
                    buf.append( (char)c );
                    continue;
                }
            }
            boolean foundCode = false;
            for ( int codeIdx=0; codeIdx<XMLENTITIES.length; codeIdx++ ) {
                String code = XMLENTITIES[codeIdx][0];
                String repl = XMLENTITIES[codeIdx][1];
                if ( c == repl.charAt(0) ) {
                    buf.append(code);
                    foundCode = true;
                }
            }
            if (foundCode) continue;
            switch (c) {
                case '\'' : buf.append("&#39;"); break;
                case ']'  : buf.append("&#93;"); break;
                case '['  : buf.append("&#91;"); break;
                case '\\' : buf.append("&#92;"); break;
                default : {
                    if ( c<32 || c>127 || encodeAll==Boolean.TRUE ) {
                        buf.append('&');
                        buf.append('#');
                        buf.append(c);
                        buf.append(';');
                    } else {
                        buf.append( (char)c );
                    }
                } break;
            }
        }
        return buf.toString();
    }


    public static String unescapeXML( final String text ) {
        StringBuilder result = new StringBuilder( text.length() );
        for ( int txtIdx=0,len=text.length(); txtIdx<len; txtIdx++ ) {
            char charAt = text.charAt(txtIdx);
            if ( charAt != '&' ) {
                result.append(charAt);
                continue;
            }
            if ( text.regionMatches(txtIdx,"&#",0,2) ) {
                try {
                    String s = text.substring(
                        txtIdx, Math.min(text.length(),txtIdx+9)
                    ).replaceFirst( "^&#(x?\\d+);.*", "$1" );
                    int n;
                    if ( s.charAt(0) == 'x' ) {
                        n = Integer.parseInt( s.substring(1), 16 );
                    } else {
                        n = Integer.parseInt(s,10);
                    }
                    txtIdx += ( 2 + s.length() );
                    result.append( (char) n );
                } catch ( Throwable t ) {
                    result.append(charAt);
                }
                continue;
            }
            boolean foundCode = false;
            for ( int codeIdx=0; codeIdx<XMLENTITIES.length; codeIdx++ ) {
                String code = XMLENTITIES[codeIdx][0];
                String repl = XMLENTITIES[codeIdx][1];
                if (text.regionMatches( true, txtIdx, code, 0, code.length() )) {
                    result.append(repl);
                    txtIdx += code.length() - 1;
                    foundCode = true;
                    break;
                }
            }
            if (foundCode) continue;
            result.append(charAt);
        }
        return result.toString();
    }
    @SuppressWarnings("unused")
    private static boolean unescapeXML_TEST_() {
        {
            String s = "~\t\0`!@#$%^&*()_ok123=+{[ }]|\\?/<,>.'\"";
            String escaped, unescaped;
            escaped = xmlSafeText(s,null);
            unescaped = unescapeXML(escaped);
            LibTest.asrt(unescaped.equals(s), "unescapeXML test 1 failed");
            escaped = xmlSafeText(s,true);
            LibTest.asrt(unescaped.equals(s), "unescapeXML test 2 failed");
            escaped = xmlSafeText(s,false);
            LibTest.asrt(unescaped.equals(s), "unescapeXML test 3 failed");
            LibTest.asrt(unescapeXML("&#x20;").equals(" "), "unescapeXML test 4 failed");
            LibTest.asrt(unescapeXML("&#92;&#92;").equals("\\\\"), "unescapeXML test 5 failed");
        }
        for ( String s : new String[]{"&","&#","&#;","&#0"} ) {
            // these are not unescape-able
            LibTest.asrt(unescapeXML(s).equals(s), "unescapeXML test 6 failed");
        }
        for ( String s : new String[]{
            "&aMp;", "&#1234;", "He read &#22;War &amp; Peace&QUOT;."
        } ) {
            // these are already escaped
            String escaped = xmlSafeText(s);
            LibTest.asrt(escaped.equals(s), "unescapeXML test 7 failed");
        }
        // make sure an html comment is escaped
        LibTest.asrt(! xmlSafeText("-->").equals("-->"), "need safety inside html comments" );
        return true;
    }


    public static String urlEncode( Map<String,Object> map ) {
        StringBuilder sb = new StringBuilder();
        for ( Map.Entry<String,Object> entry : map.entrySet() ) {
            if ( sb.length() > 0 ) sb.append("&");
            String k = entry.getKey();
            Object v = entry.getValue();
            if (v==null) continue;
            sb.append( urlEncode(k,false) );
            sb.append('=');
            sb.append( urlEncode(v.toString(),false) );
        }
        return sb.toString();
    }
    public static String urlEncode( Object obj ) {
        return urlEncode(obj,null);
    }
    public static String urlEncode( Object obj, Boolean encodeAll ) {
        if (obj==null) return "";
        if ( obj instanceof Number ) return urlEncode( obj.toString(), encodeAll );
        if ( obj instanceof char[] cArr ) return urlEncode( new String(cArr), encodeAll );
        if ( obj instanceof byte[] bArr ) return urlEncode( new String(bArr), encodeAll );
        String inputStr = obj.toString();
        final String HEX_ALPHA = "0123456789ABCDEF";
        String safeAlpha = (
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ" +
            "abcdefghijklmnopqrstuvwxyz" +
            "0123456789"
        );
        if ( encodeAll != null ) {
            if (encodeAll) {
                safeAlpha = "";
            } else {
                safeAlpha += (
                    " _*.-" + // allegedly not changed by urlEncoding
                    "!\"#$&'(),/:;<=>?@[\\]^`{|}~"
                );
            }
        }
        StringBuilder sb = new StringBuilder();
        for ( int i=0; i<inputStr.length(); i++ ) {
            char c = inputStr.charAt(i);
            if ( safeAlpha.indexOf(c) >= 0 ) {
                sb.append(c);
            } else {
                if (c>255) {
                    sb.append(URLEncoder.encode( ""+c, StandardCharsets.UTF_8 ));
                } else {
                    sb.append( "%" );
                    sb.append( HEX_ALPHA.charAt( (c>>4) & 0xF ) );
                    sb.append( HEX_ALPHA.charAt( c & 0xF ) );
                }
            }
        }
        return sb.toString();
    }
    public static String urlDecode( String s ) {
        return URLDecoder.decode(s, StandardCharsets.UTF_8 );
    }
    @SuppressWarnings("unused")
    private static boolean urlEncode_TEST_() {
        String s = "Crunchwrap Supreme� (Beef or Spicy Chicken) + tortilla chips";
        LibTest.asrt( s.equals(
            URLDecoder.decode( URLEncoder.encode(s,StandardCharsets.UTF_8), StandardCharsets.UTF_8 )
        ) );
        String encodedA = urlEncode(s,null);
        String encodedB = urlEncode(s,true);
        String encodedC = urlEncode(s,false);
        LibTest.asrtEQ( urlDecode(encodedA), s );
        LibTest.asrtEQ( urlDecode(encodedB), s );
        LibTest.asrtEQ( urlDecode(encodedC), s );
        return true;
    }


    public static String toString( byte[] bArr, int off, int len ) {
        return new String(bArr,off,len,StandardCharsets.UTF_8);
    }
    public static String toString( Object o ) {
        if (o==null) return "";
        if (o instanceof InputStream inp) {
            return new String( LibIO.readFully(inp), StandardCharsets.UTF_8 );
        }
        if ( o instanceof Reader r ) {
            return new String( LibIO.readFully(r) );
        }
        if ( o instanceof byte[] bArr ) {
            return new String( bArr, StandardCharsets.UTF_8 );
        }
        if ( o instanceof char[] cArr ) {
            return new String( cArr );
        }
        if ( o instanceof Throwable t ) {
            StringWriter sw = new StringWriter();
            PrintWriter pw = new PrintWriter(sw);
            pw.write( t.toString() );
            pw.write( '\n' );
            t.printStackTrace(pw);
            return sw.toString();
        }
        return o.toString();
    }


    /**
     * 	Removing leading and trailing whitespace,
     * 	and replaces all other whitespace with a single space.
     */
    public static String nw( CharSequence cs ) {
        return normalSpace(cs);
    }
    public static String normalSpace( CharSequence cs ) {
        String s = cs.toString();
        s = s.trim().replaceAll( "\\s+", " " );
        return s;
    }
    @SuppressWarnings("unused")
    private static boolean normalSpace_TEST_() {
        String s = """
            part1   part2
            \n part3 \t \r
        """;
        s = normalSpace(s);
        return s.equals("part1 part2 part3");
    }


    public static String onlyAlphaNum( Object o ) {
        if (o==null) return "";
        String str = o.toString();
        int i, len=str.length();
        StringBuffer buf = new StringBuffer( Math.min(10*1024,len) );
        for (i=0; i<len; i++) {
            char c = str.charAt(i);
            if (
                ( c>='a' && c<='z' ) ||
                ( c>='A' && c<='Z' ) ||
                ( c>='0' && c<='9' )
            ) {
                buf.append(c);
            }
        }
        return buf.toString();
    }
    @SuppressWarnings("unused")
    private static boolean onlyAlphaNum_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        LibTest.asrtEQ( "abc123", onlyAlphaNum("abc123") );
        LibTest.asrtEQ( "abc123", onlyAlphaNum("abc123!") );
        LibTest.asrtEQ( "abc123", onlyAlphaNum("a b@c#1$2%3^") );
        LibTest.asrtEQ( "", onlyAlphaNum("!@#$%^&*()") );
        LibTest.asrtEQ( "", onlyAlphaNum(null) );
        return true;
    }


    public static boolean isEmail( String s ) {
        if (s==null) return false;
        return s.matches("^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,6}$");
    }


    public static String unindent( String s ) {
        if (s==null) return s;
        String[] lines = s.split("\n");
        int minIndent = Integer.MAX_VALUE;
        for ( int idx=0; idx<lines.length; idx++ ) {
            String line = lines[idx];
            if ( line.trim().length()==0 ) continue;
            int indent = 0;
            while ( indent<line.length() && Character.isWhitespace(line.charAt(indent)) ) {
                indent++;
            }
            if ( indent < minIndent ) {
                minIndent = indent;
            }
        }
        if ( minIndent==0 ) return s;
        StringBuilder result = new StringBuilder();
        for ( int idx=0; idx<lines.length; idx++ ) {
            String line = lines[idx];
            if ( idx>0 ) result.append("\n");
            if ( line.length() >= minIndent ) {
                result.append( line.substring(minIndent) );
            } else {
                result.append( line );
            }
        }
        return result.toString();
    }
    @SuppressWarnings("unused")
    private static boolean unindent_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        String s = """
            line 1
              line 2
            line 3
        """;
        s = unindent(s).trim();
        String expected = """
        line 1
          line 2
        line 3""";
        return s.equals(expected);
    }


    public static String dblQuot( Object o ) { return quot(o,'"'); }
    public static String quot(Object o, String openQuot, String closeQuot) {
        if (o==null) return openQuot+closeQuot;
        String s = o.toString();
        return openQuot + s.replaceAll(closeQuot, "\\\\"+closeQuot) + closeQuot;
    }
    public static String quot(Object o, String quot) {
        return quot(o,quot,quot);
    }
    public static String quot(Object o, char quot) {
        if (o==null) return ""+quot+quot;
        String s = o.toString();
        String qStr = ""+quot;
        String qRegex = ( quot=='\\' ? "\\\\\\\\" : ( quot=='^' ? "\\\\^" : ( quot=='$' ? "\\\\$" : qStr ) ) );
        String qRepl = "\\\\"+qStr;
        return qStr + s.replaceAll(qRegex, qRepl) + qStr;
    }
    @SuppressWarnings("unused")
    private static boolean quot_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        {
            String test = quot( "ab\"cd", '"' );
            String want = "\"ab\\\"cd\"";
            LibTest.asrtEQ( want, test );
        }
        {
            String test = quot( "ab'cd", '\'' );
            String want = "'ab\\'cd'";
            LibTest.asrtEQ( want, test );
        }
        return true;
    }


    public static String[] wrapText(String text, int width) { return wrapText(text,width,true); }
    public static String[] wrapText(String text, int width, boolean force) {
        String[] words = text.trim().split("\\s+");
        if (force) {
            List<String> newWords = new ArrayList<String>();
            for (String word : words) {
                while ( word.length() > width ) {
                    newWords.add(word.substring(0, width));
                    word = word.substring(width);
                }
                if (! word.isBlank() ) newWords.add(word);
            }
            words = newWords.toArray(new String[0]);
        }
        List<String> lines = new ArrayList<String>();
        StringBuilder line = new StringBuilder();
        for (String word : words) {
            boolean needSpace = line.length() > 0;
            if (line.length() + word.length() + (needSpace ? 1 : 0) > width) {
                if (! line.isEmpty() ) lines.add(line.toString());
                line.setLength(0);
                needSpace = false;
            }
            if (needSpace) line.append(' ');
            line.append(word);
        }
        if (line.length() > 0)
            lines.add(line.toString());
        return lines.toArray(new String[0]);
    }
    @SuppressWarnings("unused")
    private static boolean wrapText_TEST_() {
        String input;
        String[] output, expected;
        int lineLen = 10;
        { // test with force
            input = """
                This is a long string of text, which needs to \t be wrapped,\n
                IncludingThisVeryLongWord AndThisLongWordAlso.
            """;
            output = wrapText(input,lineLen,true);
            expected = new String[]{
                "This is a", "long", "string of", "text,", "which", "needs to", "be",
                "wrapped,", "IncludingT", "hisVeryLon", "gWord", "AndThisLon", "gWordAlso."
            };
            LibTest.asrtEQ(Arrays.asList(expected), Arrays.asList(output));
        }
        { // test without force
            input = """
                This is a long string of text, which needs to \t be wrapped,\n
                IncludingThisVeryLongWord AndThisLongWordAlso.
            """;
            output = wrapText(input,lineLen,false);
            expected = new String[]{
                "This is a", "long", "string of", "text,", "which", "needs to", "be",
                "wrapped,", "IncludingThisVeryLongWord", "AndThisLongWordAlso."
            };
            LibTest.asrtEQ(Arrays.asList(expected), Arrays.asList(output));
        }
        return true;
    }



    public static String rpad( String str, int len, String pad ) {
        if ( str==null ) str = "";
        if ( pad==null || pad.isEmpty() ) pad = " ";
        StringBuilder sb = new StringBuilder(str);
        while ( sb.length() < len ) {
            sb.append(pad);
        }
        return sb.toString().substring(0, len);
    }
    @SuppressWarnings("unused")
    private static boolean rpad_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        LibTest.asrtEQ( "hello     ", rpad("hello", 10, " ") );
        LibTest.asrtEQ( "hello.....", rpad("hello", 10, ".") );
        LibTest.asrtEQ( "hello12312", rpad("hello", 10, "123") );
        LibTest.asrtEQ( "hellohello", rpad("hellohello", 10, " ") );
        LibTest.asrtEQ( "hellohello", rpad("hellohelloworld", 10, " ") );
        return true;
    }


    public static String centerPad( String str, int len ) {
        if ( str==null ) str = "";
        if ( str.length() >= len ) return str;
        int totalPad = len - str.length();
        int leftPad = totalPad / 2;
        int rightPad = totalPad - leftPad;
        return " ".repeat(leftPad) + str + " ".repeat(rightPad);
    }
    @SuppressWarnings("unused")
    private static boolean centerPad_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        LibTest.asrtEQ( "  hello   ", centerPad("hello", 10) );
        LibTest.asrtEQ( "   hi   ", centerPad("hi", 8) );
        LibTest.asrtEQ( "world", centerPad("world", 5) );
        LibTest.asrtEQ( "toolong", centerPad("toolong", 5) );
        return true;
    }


    public static String evalUrlTemplate( String template, Map<?,?> vars ) {
        return evalTemplate(template,"\\$","\\$",vars);
    }
    public static String evalTemplate( File templateFile, Map<?,?> vars ) throws IOException {
        return evalTemplate( LibFile.file2string(templateFile), vars );
    }
    public static String evalTemplate( String template, Map<?,?> vars ) {
        return evalTemplate(template,"\\{\\{\\s*","\\s*\\}\\}",vars);
    }
    public static String evalTemplate( String template, String openRegex, String closeRegex, Map<?,?> vars ) {
        if (template==null) return null;
        if (vars==null) return template;
        Matcher mat = null;
        try {
            mat = Pattern.compile(
                openRegex +"\\s*(.*?)\\s*"+ closeRegex
            ).matcher(template);
        } catch ( PatternSyntaxException e ) {
            L0g.log(e);
            return template;
        }
        ScriptEngineManager manager = null;
        ScriptEngine engine = null;
        StringBuffer result = new StringBuffer();
        Map<String,Object> normalVars = new LinkedHashMap<>();
        for ( Map.Entry<?,?> entry : vars.entrySet() ) {
            Object key = entry.getKey();
            if (key==null) continue;
            Object val = entry.getValue();
            normalVars.put( key.toString(), val );
        }
        while ( mat.find() ) {
            //String wholeExpression = mat.group();
            String varName = mat.group(1);
            String keyName = Lib.findKey(varName,normalVars);
            Object value = null;
            if ( keyName!=null && vars.containsKey(keyName) ) {
                value = vars.get(keyName);
            } else {
                if (manager==null) {
                    manager = new ScriptEngineManager();
                    engine = manager.getEngineByName("js");
                    if ( engine==null ) {
                        List<ScriptEngineFactory> factories = manager.getEngineFactories();
                        for (ScriptEngineFactory factory : factories) {
                            engine = factory.getScriptEngine();
                            break;
                        }
                    }
                    if (engine!=null) {
                        Bindings bindings = engine.createBindings();
                        for( Map.Entry<String,Object> entry : normalVars.entrySet() ) {
                            bindings.put(entry.getKey(), entry.getValue());
                        }
                        engine.setBindings( bindings, ScriptContext.ENGINE_SCOPE );
                    }
                }
                try {
                    value = engine==null ? null : engine.eval(varName);
                } catch ( ScriptException e ) {
                    L0g.log(e);
                }
            }
            if (value==null) value="";
            mat.appendReplacement( result, Matcher.quoteReplacement( value.toString() ) );
        }
        mat.appendTail(result);
        return result.toString();
    }
    @SuppressWarnings("unused")
    private static boolean evalTemplate_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        { // mustache-like
            String template = "The {{ NOUN }} in {{ COUNTRY }} stays mainly on the {{ pLaCe }}.";
            Map<String,Object> map = new LinkedHashMap<>();
            map.put("nOuN","rain");
            map.put("country","Spain");
            map.put("place","plain");
            String result = evalTemplate(template,map);
            String expected = "The rain in Spain stays mainly on the plain.";
            LibTest.asrt(result.equals(expected), "evalTemplate jsp-like test failed");
        }
        { // url-safe
            String template = "The $NOUN$ in $cOuNtrY$ stays mainly on the $PLACE$.";
            Map<String,Object> map = new LinkedHashMap<>();
            map.put("noun","rain");
            map.put("country","Spain");
            map.put("PLACE","plain");
            String result = evalUrlTemplate(template,map);
            String expected = "The rain in Spain stays mainly on the plain.";
            LibTest.asrt(result.equals(expected), "evalUrlTemplate test failed");
        }
        return true;
    }


    public static void main( String[] args ) { LibTest.testClass(); }
}
