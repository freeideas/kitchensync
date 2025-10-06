package jLib;
import java.io.*;
import java.net.*;
import java.util.*;
import java.security.*;
import java.lang.reflect.*;
import java.time.*;
import java.time.format.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;
import java.util.stream.*;
import javax.net.ssl.*;
import javax.crypto.SecretKeyFactory;
import javax.crypto.spec.PBEKeySpec;



public class Lib {
    public static final java.lang.ref.Cleaner cleaner = java.lang.ref.Cleaner.create(); // so everyone can use one cleaner instead of having many
    public static final String CRLF = "\r\n";



    /**
     * Delegates to LibApp.loadCreds() to ensure consistent credential loading
     */
    public static Object loadCreds() {
        return LibApp.loadCreds();
    }
    @SuppressWarnings("unused")
    private static boolean loadCreds_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        LibTest.asrt(!isEmpty( loadCreds() ));
        return true;
    }



    public static String normalizePath( String path ) {
        if (path==null) return null;
        boolean startsWithSlash = path.startsWith("/") || path.startsWith("\\");
        boolean isURL = path.matches("\\w+://.*");
        String[] parts = path.split("[/\\\\]");
        List<String> result = new ArrayList<>();
        for (String part : parts) {
            if (part.isEmpty() || part.equals(".")) continue;
            if ( part.equals("..") && !result.isEmpty() ) {
                    result.remove(result.size() - 1);
                    continue;
            }
            result.add(part);
        }
        String resStr = (startsWithSlash ? "/" : "") + String.join("/", result);
        if (isURL) resStr = resStr.replaceFirst("^(\\w+):/+", "$1://");
        return resStr;
    }
    @SuppressWarnings("unused")
    private static boolean normalizePath_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        LibTest.asrtEQ( normalizePath("a/b/c"), "a/b/c" );
        LibTest.asrtEQ( normalizePath("//a/b/c/"), "/a/b/c" );
        LibTest.asrtEQ( normalizePath("sftp://a.com/b//c/..//"), "sftp://a.com/b" );
        LibTest.asrtEQ( normalizePath(""), "" );
        LibTest.asrtEQ( normalizePath(null), null );
        return true;
    }



    public static ServerSocket createServerSocket(
        int port, boolean tls, File cert, String keystorePassword, String bindAddress
    ) throws IOException {
        InetAddress addr = null;
        if ( bindAddress!=null ) {
            try { addr = InetAddress.getByName(bindAddress); }
            catch ( Exception e ) { addr = null; }
        }
        if (!tls) {
            if ( addr!=null ) return new ServerSocket( port, 50, addr );
            return new ServerSocket( port, 50 );
        }
        try {
            SSLContext sslContext = SSLContext.getInstance("TLSv1.3");
            if (cert != null) {
                // Load the keystore from the provided certificate file
                KeyStore keyStore = KeyStore.getInstance("PKCS12");
                try (FileInputStream fis = new FileInputStream(cert)) {
                    keyStore.load(fis, keystorePassword.toCharArray());
                }
                KeyManagerFactory keyManagerFactory = KeyManagerFactory.getInstance(KeyManagerFactory.getDefaultAlgorithm());
                keyManagerFactory.init(keyStore, keystorePassword.toCharArray());
                sslContext.init(keyManagerFactory.getKeyManagers(), null, null);
            } else {
                sslContext.init(null, null, null);
            }
            SSLServerSocketFactory sslServerSocketFactory = sslContext.getServerSocketFactory();
            if ( addr!=null ) return sslServerSocketFactory.createServerSocket( port, 50, addr );
            return sslServerSocketFactory.createServerSocket( port, 50 );
        }
        catch (IOException ioe) { throw ioe; }
        catch (Exception e) { throw new IOException(e); }
    }
    @SuppressWarnings("unused")
    private static boolean createServerSocket_TEST_( boolean findLineNumber ) throws Exception {
        if (findLineNumber) throw new RuntimeException();
        // Test null bindAddress (all interfaces)
        try ( ServerSocket ss = createServerSocket( 19999, false, null, null, null ) ) {
            LibTest.asrt( ss.getLocalSocketAddress().toString().contains("0.0.0.0") );
        }
        // Test localhost
        try ( ServerSocket ss = createServerSocket( 19999, false, null, null, "localhost" ) ) {
            LibTest.asrt( ss.getLocalSocketAddress().toString().contains("127.0.0.1") );
        }
        // Test specific IP
        try ( ServerSocket ss = createServerSocket( 19999, false, null, null, "127.0.0.1" ) ) {
            LibTest.asrt( ss.getLocalSocketAddress().toString().contains("127.0.0.1") );
        }
        // Test invalid address (fallback to all interfaces)
        try ( ServerSocket ss = createServerSocket( 19999, false, null, null, "badaddress" ) ) {
            LibTest.asrt( ss.getLocalSocketAddress().toString().contains("0.0.0.0") );
        }
        return true;
    }



    /**
     * If both are Maps, then copies entries from copyFrom into copyInto, and returns copyInto.
     * If both are Lists, then copies each element of copyFrom to copyInto at the same index, and returns copyInto.
     * Otherwise, returns copyFrom.
     * If modifyCopyInto is true but some of the underlying data structures are immutable,
     * then a new copy is created as needed.
     **/
    public static Object merge( Object copyFrom, Object copyInto, Boolean modifyCopyInto ) {
        if (modifyCopyInto==null) modifyCopyInto=false;
        if ( copyFrom instanceof Map && copyInto instanceof Map ) {
            @SuppressWarnings("unchecked")
            Map<Object,Object> fromMap = (Map<Object,Object>) copyFrom;
            @SuppressWarnings("unchecked")
            Map<Object,Object> intoMap = (Map<Object,Object>) copyInto;
            for ( Map.Entry<Object,Object> entry : fromMap.entrySet() ) {
                Object key = entry.getKey();
                Object value = entry.getValue();
                if ( intoMap.containsKey(key) ) value = merge( value, intoMap.get(key), modifyCopyInto );
                try {
                    intoMap.put(key,value);
                } catch ( RuntimeException re ) {
                    Map<Object,Object> newIntoMap = new LinkedHashMap<>();
                    newIntoMap.putAll(intoMap);
                    intoMap = newIntoMap;
                    intoMap.put(key,value);
                }
            } return intoMap;
        }
        if ( copyFrom instanceof List && copyInto instanceof List ) {
            @SuppressWarnings("unchecked")
            List<Object> fromList = (List<Object>) copyFrom;
            @SuppressWarnings("unchecked")
            List<Object> intoList = (List<Object>) copyInto;
            for (int i=0; i<fromList.size(); i++) {
                Object value = fromList.get(i);
                if ( i<intoList.size() ) value = merge( value, intoList.get(i), modifyCopyInto );
                try {
                    if ( i< intoList.size() ) { intoList.set(i,value); }
                    else { intoList.add(value); }
                } catch ( RuntimeException re ) {
                    List<Object> newIntoList = new ArrayList<>();
                    newIntoList.addAll(intoList);
                    intoList = newIntoList;
                    if ( i< intoList.size() ) { intoList.set(i,value); }
                    else { intoList.add(value); }
                }
            } return intoList;
        }
        return copyFrom;
    }
    @SuppressWarnings("unused")
    private static boolean merge_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        { // simple map
            Map<Object,Object> srcMap = JsonDecoder.decodeMap("""
                { "one":1, "two":2, "three":3 }
            """);
            Map<Object,Object> tgtMap = JsonDecoder.decodeMap("""
                { "three":0, "four":4, "five":5 }
            """);
            Object result = merge(srcMap,tgtMap,true);
            Map<Object,Object> expected = JsonDecoder.decodeMap("""
                { "one":1, "two":2, "three":3, "four":4, "five":5 }
            """);
            LibTest.asrtEQ(expected,result);
        }
        { // simple lists
            Object src = JsonDecoder.decode("""
                [1,2,3,4]
            """);
            Object dst = JsonDecoder.decode("""
                [-3,-2]
            """);
            Object result = merge(src,dst,true);
            Object expected = JsonDecoder.decode("""
                [1,2,3,4]
            """);
            LibTest.asrtEQ(expected,result);
        }
        { // mixture of lists and maps
            Object src = JsonDecoder.decode("""
                { "a":[1,2,3], "b":2, "c":{1:"one",2:"two"} }
            """);
            Object dst = JsonDecoder.decode("""
                { "a":[1,2,3,4], "b":[1,2], "c":{3:"three",1:"ONE"}, "d":4 }
            """);
            Object result = merge(src,dst,true);
            Object expected = JsonDecoder.decode("""
                { "a":[1,2,3,4], "b":2, "c":{3:"three",1:"one",2:"two"}, "d":4 }
            """);
            String expStr = JsonEncoder.encode(expected);
            String resStr = JsonEncoder.encode(result);
            LibTest.asrtEQ(expStr,resStr);
        }
        return true;
    }



    public static String randToken() {
        return randToken( null, 8, false );
    }
    public static String randToken( String prefix, int suffixLen, boolean digitsOnly  ) {
        if (suffixLen<=0) suffixLen=8;
        final String alpha = digitsOnly ? "0123456789" : "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        int alphaLen = alpha.length();
        Random rand = new Random( System.currentTimeMillis() );
        StringBuilder sb = new StringBuilder();
        if (prefix!=null) sb.append(prefix);
        if (Lib.isEmpty(prefix) && !digitsOnly) {
            sb.append(alpha.charAt( rand.nextInt(26) ));
            suffixLen--;
        }
        while ( suffixLen-- > 0 ) {
            sb.append(alpha.charAt( rand.nextInt(alphaLen) ));
        }
        return sb.toString();
    }
    @SuppressWarnings("unused")
    private static boolean randToken_TEST_() {
        String token = randToken( null, 8, false );
        if ( token.length()!=8 ) throw new RuntimeException("fail len");
        token = randToken( null, 8, true );
        if ( token.length()!=8 ) throw new RuntimeException("fail len");
        token = randToken( "abc", 8, false );
        if ( token.length()!=11 ) throw new RuntimeException("fail len");
        token = randToken( "abc", 8, true );
        if ( token.length()!=11 ) throw new RuntimeException("fail len");
        return true;
    }



    public static String toISO8601( String utcTimestamp ) {
        utcTimestamp = utcTimestamp.replaceAll("[^0-9]","");
        final int NEED_DIGIT_COUNT = 23;
        if ( utcTimestamp.length() < NEED_DIGIT_COUNT ) {
            utcTimestamp = utcTimestamp + "0".repeat( NEED_DIGIT_COUNT - utcTimestamp.length() );
        } else if ( utcTimestamp.length() > NEED_DIGIT_COUNT ) {
            utcTimestamp = utcTimestamp.substring(0,NEED_DIGIT_COUNT);
        }
        utcTimestamp = ( // add the T and everything else Instant.parse will expect
            utcTimestamp.substring(0,4) +"-"+
            utcTimestamp.substring(4,6) +"-"+
            utcTimestamp.substring(6,8) +"T"+
            utcTimestamp.substring(8,10) +":"+
            utcTimestamp.substring(10,12) +":"+
            utcTimestamp.substring(12,14) +"."+
            utcTimestamp.substring(14) +"Z"
        );
        return utcTimestamp;
    }



    public static long microsSinceEpoch( String utcTimestamp ) {
        utcTimestamp = toISO8601(utcTimestamp);
        Instant instant = Instant.parse(utcTimestamp);
        return toMicrosSinceEpoch(instant);
    }
    @SuppressWarnings("unused")
    private static boolean microsSinceEpoch_TEST_( boolean findLineNumber ) throws Exception {
        if (findLineNumber) throw new RuntimeException();
        long now = System.currentTimeMillis() * 1000 + 1L;
        long micros = microsSinceEpoch( timeStamp(now) );
        long diff = Math.abs(micros - now);
        LibTest.asrtEQ(0, diff);
        return true;
    }



    public static long toMicrosSinceEpoch( Instant instant ) {
        return instant.getEpochSecond() * 1000000 + instant.getNano() / 1000;
    }
    @SuppressWarnings("unused")
    private static boolean toMicrosSinceEpoch_TEST_( boolean findLineNumber ) throws Exception {
        if (findLineNumber) throw new RuntimeException();
        Instant now = Instant.now();
        long millisSinceEpoch = System.currentTimeMillis();
        long microsSinceEpoch = toMicrosSinceEpoch(now);
        long expected = millisSinceEpoch * 1000;
        long diff = Math.abs(microsSinceEpoch - expected);
        LibTest.asrt( diff < 2000 ); // i.e. within 2 millis
        return true;
    }



    public static String getRemoteAddr( Socket sock ) {
        InetAddress inetAddress = sock.getInetAddress();
        String ip = inetAddress.getHostAddress();
        if (inetAddress instanceof Inet6Address) {
            byte[] addr = inetAddress.getAddress();
            if (addr.length == 16) {
                boolean isIPv4Mapped = true;
                for (int i = 0; i < 10; i++) {
                    if (addr[i] != 0) { isIPv4Mapped = false; break; }
                }
                if (isIPv4Mapped && addr[10] == (byte)0xff && addr[11] == (byte)0xff) {
                    ip = (addr[12] & 0xff) + "." + (addr[13] & 0xff) + "." +
                         (addr[14] & 0xff) + "." + (addr[15] & 0xff);
                }
            }
        }
        return ip;
    }



    /**
     * NOTE: This will return true if either given path is the parent of the other.
     */
    public static boolean isParentChildPath( String pathA, String pathB ) {
        if ( pathA==null || pathB==null ) return false;
        String normA = normalizePath(pathA);
        String normB = normalizePath(pathB);
        if (! normA.endsWith("/") ) normA += "/";
        if (! normB.endsWith("/") ) normB += "/";
        return normA.startsWith(normB) || normB.startsWith(normA);
    }
    @SuppressWarnings("unused")
    private static boolean isParentChildPath_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        LibTest.asrt( isParentChildPath("a/b/c","a/b/c/d") );
        LibTest.asrt( isParentChildPath("a/b\\c/d","a/b//c") );
        LibTest.asrt( isParentChildPath("a/b/c","a/b/c") );
        LibTest.asrt( isParentChildPath("/a/b/c","\\a/b/c/..") );
        LibTest.asrt( isParentChildPath("a/b/./c/..","a/b/c") );
        LibTest.asrt(! isParentChildPath("a/b/c","a/b/c/../d") );
        LibTest.asrt( isParentChildPath("a/b/c/../d","a/b/d") );
        LibTest.asrt(! isParentChildPath("a/b/c","a/b/c/../../d") );
        return true;
    }



    public static byte[] toBytes( InputStream inp, boolean close ) throws IOException {
        try {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            copy(inp,out);
            return out.toByteArray();
        } finally {
            if (close) try{ inp.close(); }catch(Exception ignore){}
        }
    }



    @SuppressWarnings("BusyWait")
    public static long copy( Reader inp, Writer... outs )
        throws IOException
    {
        long totalCount = 0;
        char[] buf = new char[8192];
        int count = -1;
        try {
            count = inp.read(buf);
            if (count>0) totalCount+=count;
        } catch ( Throwable t ) {}
        while (count>=0) {
            if (count==0) {
                try { Thread.sleep(100); } catch ( InterruptedException ie ) {}
            } else {
                for (Writer out:outs) if(out!=null) out.write(buf,0,count);
            }
            count = inp.read(buf);
        }
        for (Writer out:outs) if(out!=null) out.flush();
        return totalCount;
    }
    public static long copy( InputStream inp, OutputStream... outs )
        throws IOException
    {
        int MAX_BUF_SIZE = 32*1024;
        long totalCount = 0;
        byte[] buf = new byte[512];
        while (true) {
            long nanoTimer = System.nanoTime();
            int count = inp.read(buf);
            nanoTimer = System.nanoTime() - nanoTimer;
            if ( count>0 && nanoTimer > count*1000*1000 ) { // more than 1 milli per byte
                long millis = nanoTimer / (1000*1000);
                if (millis>1000)Log.logOnce(
                    "Lib.copy_slowRead", "copy() took "+millis+" millis to read "+count+" bytes", 5000
                );
            }
            if (count<0) break;
            if (count==0) {
                try {
                    Log.logOnce( "copy() read 0 bytes, sleeping" );
                    Thread.sleep(100);
                } catch ( InterruptedException ie ) {}
                continue;
            }
            nanoTimer = System.nanoTime();
            for (OutputStream out : outs) if(out!=null) out.write(buf,0,count);
            nanoTimer = System.nanoTime() - nanoTimer;
            if ( count>0 && nanoTimer > count*1000*1000 ) { // more than 1 milli per byte
                long millis = nanoTimer / (1000*1000);
                if (millis>1000)Log.logOnce(
                    "Lib.copy_slowWrite", "copy() took "+millis+" millis to write "+count+" bytes", 5000
                );
            }
            totalCount += count;
            if (count==buf.length && buf.length<MAX_BUF_SIZE) {
                buf = new byte[buf.length*2];
            }
        }
        long nanoTimer = System.nanoTime();
        for (OutputStream out : outs) if(out!=null) out.flush();
        nanoTimer = System.nanoTime() - nanoTimer;
        if ( nanoTimer > 500*1000*1000 ) {
            if ( Log.logOnce("copy() took > 500 millis to flush") ) {
                long millis = nanoTimer / (1000*1000);
                Log.log( "copy() took "+millis+" millis to flush" );
            }
        }
        return totalCount;
    }
    @SuppressWarnings("unused")
    private static boolean copy_slow_TEST_() throws Exception {
        // create a fake OutputStream to similuate a slow write
        OutputStream slowOut = new OutputStream() {
            public void write( int b ) throws IOException {
                try { Thread.sleep(1); } catch ( InterruptedException ie ) {}
            }
        };
        byte[] bArr = new byte[1000];
        for (int i=0; i<bArr.length; i++) bArr[i] = (byte)i;
        InputStream inp = new ByteArrayInputStream(bArr);
        copy(inp,slowOut);
        LibTest.asrt(!( // one of these messages should have been logged already
            Log.logOnce( "", "Lib.copy_slowRead", 5000 ) &&
            Log.logOnce( "", "Lib.copy_slowWrite", 5000 )
        ));
        return true;
    }



    public static InputStream toInputStream( Object o ) throws IOException {
        if (o==null) o="";
        if ( o instanceof URL url ) return url.openStream();
        if ( o instanceof byte[] bArr ) return new ByteArrayInputStream(bArr);
        if ( o instanceof InputStream ) return (InputStream)o;
        if ( o instanceof Reader r ) return LibIO.readerInputStream(r);
        if ( o instanceof File ) {
            File f = (File)o;
            if ( f.isFile() ) return new FileInputStream(f);
            o = "";
        }
        if ( o instanceof CharSequence s ) return new ByteArrayInputStream( s.toString().getBytes() );
        return toInputStream( JsonEncoder.encode(o) );
    }



    /**
    * Appends to the given file in a reasonably synchronized way. Not fast.
    **/
    public static Throwable append2file( File f, Object data ) {
        Closeable toClose = null;
        try {
            String canonicalPath = f.getCanonicalPath();
            synchronized ( canonicalPath.intern() ) {
                try (
                    FileOutputStream fout = new FileOutputStream(f,true);
                    BufferedOutputStream bout = new BufferedOutputStream(fout);
                ) {
                    InputStream inp = null;
                    if ( data instanceof InputStream dataInp ) {
                        inp = dataInp;
                    } else {
                        inp = toInputStream(data);
                        toClose = inp;
                    }
                    copy(inp,bout);
                    bout.flush();
                    fout.flush();
                }
            }
            return null;
        } catch ( Throwable t ) {
            return t;
        } finally {
            if (toClose!=null) {
                try{ toClose.close(); }catch(Throwable ignore){}
            }
        }
    }
    @SuppressWarnings("unused")
    private static boolean append2file_TEST_() throws IOException {
        File f = File.createTempFile( "append2file_TEST_", ".tmp" );
        f.deleteOnExit();
        Throwable t = append2file(f,"Hello, world!");
        if (t!=null) throw new RuntimeException(t);
        t = append2file(f," Goodbye.");
        if (t!=null) throw new RuntimeException(t);
        // LibTest.asrtEQ( "Hello, world! Goodbye.", file2string(f,null) ); // file2string removed
        LibTest.asrt( f.delete() );
        return true;
    }



    public static boolean isText( byte[] bArr ) {
        if ( Lib.isEmpty(bArr) ) return true; // empty file can be processed as text
        for (int i=0; i<bArr.length; i++) {
            byte b = bArr[i];
            if ( b>126 || ( b<32 && b!=9 && b!=10 && b!=13 ) ) break;
            if ( i == bArr.length-1 ) return true; // all usASCII7
        }
        try { // try to decode as with platform default character encoding
              // ^ (always UTF-8 in recent versions of java, unless deliberately configured otherwise)
            String s = new String(bArr);
            if ( Lib.isEmpty(s) ) return false; // bytes in, nothing out: not normal text
        } catch (Throwable t) {
            return false;
        }
        return true; // if we get this far, it can at least pass for text
    }
    public static boolean isBinary( byte[] bArr ) {
        return ! isText(bArr);
    }



    @SuppressWarnings({"rawtypes","unchecked"})
    public static <T> Iterable<T> asIterable( Iterator<T> it ) {
        return new Iterable() { public Iterator iterator() { return it; } };
    }
    @SuppressWarnings("unused")
    private static boolean asIterable_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        Iterator<?> it = List.of( "whatev" ).iterator();
        Iterable<?> itbl = asIterable(it);
        for ( Object o : itbl ) LibTest.asrtEQ( "whatev", o );
        return true;
    }



    public static String thisMethodName() {
        List<String> stackTrace = getStackTrace();
        String mostRecentCall = stackTrace.get(0);
        String methodName = mostRecentCall.replaceAll( "(.*)\\.([^\\.]*)\\:(.*)", "$2" );
        return methodName;
    }
    public static String thisClassName() {
        List<String> stackTrace = getStackTrace();
        String mostRecentCall = stackTrace.get(0);
        String className = mostRecentCall.replaceAll( "(.*)\\.([^\\.]*)\\:(.*)", "$1" );
        return className;
    }
    public static List<String> getStackTrace() {
        return getStackTrace(null,true,0);
    }
    public static List<String> getStackTrace( boolean removeLibraryCalls ) {
        return getStackTrace(null,removeLibraryCalls,0);
    }
    public static List<String> getStackTrace( Throwable t, boolean removeLibraryCalls ) {
        return getStackTrace(t,removeLibraryCalls,0);
    }
    public static List<String> getStackTrace( Throwable t, boolean removeLibraryCalls, int skipRecent ) {
        StackTraceElement[] stes = t==null ? Thread.currentThread().getStackTrace() : t.getStackTrace();
        ArrayList<String> traceList = new ArrayList<>(50);
        String thisClassName = Lib.class.getName();
        for (int i=0; i<stes.length; i++) {
            StackTraceElement ste = stes[i];
            String clas = ste.getClassName();
            if ( removeLibraryCalls && (
                clas.startsWith(thisClassName) ||
                clas.startsWith("jdk.") ||
                clas.startsWith("java.") ||
                clas.startsWith("javax.") ||
                clas.startsWith("sun.") ||
                clas.startsWith("org.apache.") ||
                clas.startsWith("net.sourceforge.") ||
                clas.startsWith("oracle.") ||
                clas.startsWith("org.sqlite.") ||
                clas.startsWith("bsh.")
            )) continue;
            if ( (--skipRecent) >= 0 ) continue;
            traceList.add(
                ste.getClassName()+'.'+ste.getMethodName()+":"+ste.getLineNumber()
            );
        }
        traceList.trimToSize();
        return traceList;
    }



    public static boolean isPortListening( int port ) {
        return isPortListening("localhost", port);
    }
    public static boolean isPortListening( String  urlStr ) {
        // I wish this arg could be a proper java.net.URL, but it doesn't know how to handle default ports
        URL url = new Url(urlStr).url;
        int port = url.getPort();
        if (port<=0) port = url.getDefaultPort();
        return isPortListening( url.getHost(), port );
    }
    public static boolean isPortListening( String host, int port ) {
        try {
            Socket s = new Socket(host,port);
            s.close();
            return true;
        } catch (IOException e) {
            return false;
        }
    }



    public static String formatException( String msg ) {
        try { throw new RuntimeException(msg); } catch ( Throwable t ) { return formatException(t); }
    }
    public static String formatException( Throwable t ) {
        t = getRootCause(t);
        String s = getStackTrace(t,true).toString();
        s = s.replaceAll("\\s","");
        s = t.getClass().getSimpleName() +":"+ t.getMessage() +" @"+ s;
        return s;
    }
    public static Throwable getRootCause( Throwable t ) {
        if (t==null) return null;
        Throwable t2 = t.getCause();
        if ( t2!=null && t2!=t ) return getRootCause(t2);
        return t;
    }



    @SuppressWarnings({"rawtypes","unchecked"})
    public static Object put( Object data, Collection keys, Object value ) {
        LinkedList keyList = new LinkedList(keys);
        while (! keys.isEmpty() ) {
            Object key = keyList.removeFirst();
            if ( data instanceof Map m ) {
                if ( keyList.isEmpty() ) return m.put(key,value);
                if ( m.containsKey(key) ) {
                    data = m.get(key);
                } else {
                    if ( keyList.get(0) instanceof Integer ) {
                        data = new ArrayList();
                        m.put(key,data);
                    } else {
                        data = new LinkedHashMap();
                        m.put(key,data);
                        data = m.get(key); // in case put did something tricky, as with persistent Maps
                    }
                }
            } else
            if ( data instanceof List lst ) {
                Integer index = null;
                String s = key.toString();
                index = Integer.parseInt(s);
                if ( index < 0 ) index = lst.size() + index;
                while ( index >= lst.size() ) lst.add(null);
                if ( keyList.isEmpty() ) return lst.set(index,value);
                data = lst.get(index);
                if (data==null) {
                    data = ( keyList.get(0) instanceof Integer ) ? new ArrayList() : new LinkedHashMap<>();
                    lst.set(index,data);
                    data = lst.get(index); // in case set did something tricky, as with persistent Lists
                }
            } else {
                throw new RuntimeException( "can't put "+keys+" into "+data );
            }
        }
        throw new RuntimeException();
    }
    @SuppressWarnings("unused")
    private static boolean put_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        Object tree = new LinkedHashMap<Object,Object>();
        { // build the tree
            put( tree, List.of("a",2,"c"), "ok" );
            String result = JsonEncoder.encode(tree);
            String expected = JsonEncoder.encode( JsonDecoder.decode( """
                { "a" : [ null, null, {"c":"ok"} ] }
            """ ) );
            LibTest.asrtEQ(result,expected);
        }
        { // modify the tree
            Object wasVal = put(
                tree, List.of("a",2,"c"),
                new LinkedHashMap<Object,Object>( Map.of("whatev",3) )
            );
            LibTest.asrtEQ( wasVal, "ok" );
            String result = JsonEncoder.encode(tree);
            String expected = JsonEncoder.encode( JsonDecoder.decode( """
                { "a" : [ null, null, {"c":{"whatev":3}} ] }
            """ ) );
            LibTest.asrtEQ(result,expected);
        }
        return true;
    }



    public static Long toLong( Object o ) {
        if ( o instanceof Long l0ng ) return l0ng;
        if ( o instanceof Number n ) return n.longValue();
        if ( o instanceof CharSequence s ) {
            try {
                return Long.parseLong( s.toString() );
            } catch ( NumberFormatException ignore ) {}
        }
        return null;
    }
    public static Float toFloat( Object o ) {
        if ( o instanceof Float f ) return f;
        if ( o instanceof Number n ) return n.floatValue();
        if ( o instanceof CharSequence s ) {
            try {
                return Float.parseFloat( s.toString() );
            } catch ( NumberFormatException ignore ) {}
        }
        return null;
    }
    public static Double toDouble( Object o ) {
        if ( o instanceof Double d ) return d;
        if ( o instanceof Number n ) return n.doubleValue();
        if ( o instanceof CharSequence s ) {
            try {
                return Double.parseDouble( s.toString() );
            } catch ( NumberFormatException ignore ) {}
        }
        return null;
    }
    public static Integer toInteger( Object o ) {
        if ( o instanceof Integer i ) return i;
        if ( o instanceof Number n ) return n.intValue();
        if ( o instanceof CharSequence s ) {
            try {
                return Integer.parseInt( s.toString() );
            } catch ( NumberFormatException ignore ) {}
        }
        return null;
    }



    /**
     * Uses PBEKeySpec to hash a password.
     * Returns a 3-part string, separated by ":"
     * 1) The number of hashing iterations that were performed, in base 36
     * 2) The salt used, in base 64
     * 3) The hashed password, in base 64
     * minStrengthMillis is not exactly a "strength" per se,
     * but requires a number of hashing iterations that takes at least this many milliseconds to complete.
    **/
    public static String hashPassword( String password, int minStrengthMillis ) {
        class InnerMethods {
            private static byte[] hashPassword(char[] password, byte[] salt, int iterations)
                throws NoSuchAlgorithmException, java.security.spec.InvalidKeySpecException
            {
                PBEKeySpec spec = new PBEKeySpec(password, salt, iterations, 64 * 8);
                SecretKeyFactory skf = SecretKeyFactory.getInstance("PBKDF2WithHmacSHA1");
                return skf.generateSecret(spec).getEncoded();
            }
            private static byte[] getSalt() throws NoSuchAlgorithmException {
                SecureRandom sr = SecureRandom.getInstanceStrong();
                byte[] salt = new byte[16];
                sr.nextBytes(salt);
                return salt;
            }
        }
		try {
			byte[] salt = InnerMethods.getSalt();
			char[] chars = password.toCharArray();
			byte[] hash;
			int iterations = 512;
			while (true) {
				long startTime = System.currentTimeMillis();
				hash = InnerMethods.hashPassword(chars, salt, iterations);
				if ( System.currentTimeMillis()-startTime > minStrengthMillis ) break;
				iterations += ( iterations / 2 );
			}
			String saltBase64 = Base64.getEncoder().encodeToString(salt);
			String hashBase64 = Base64.getEncoder().encodeToString(hash);
			return Integer.toString(iterations, 36) +":"+ saltBase64 +":"+ hashBase64;
		} catch ( Throwable t ) {
			if ( t instanceof RuntimeException ) throw (RuntimeException)t;
			throw new RuntimeException(t);
		}
    }
    public static String hashPassword( String password ) { return hashPassword(password,100); }
    public static boolean verifyPassword( String password, String hashedPassword ) {
        if (password==null) return false;
		try {
			String[] parts = hashedPassword.split(":");
			int iterations = Integer.parseInt(parts[0], 36);
			byte[] salt = Base64.getDecoder().decode(parts[1]);
			byte[] storedHash = Base64.getDecoder().decode(parts[2]);
			PBEKeySpec spec = new PBEKeySpec(password.toCharArray(), salt, iterations, storedHash.length * 8);
			SecretKeyFactory skf = SecretKeyFactory.getInstance("PBKDF2WithHmacSHA1");
			byte[] testHash = skf.generateSecret(spec).getEncoded();
			int diff = storedHash.length ^ testHash.length;
			for (int i = 0; i < storedHash.length && i < testHash.length; i++) {
				diff |= storedHash[i] ^ testHash[i];
			}
			return diff == 0;
		} catch ( Throwable fail ) { return false; }
    }
	@SuppressWarnings("unused")
    private static boolean hashPassword_TEST_() {
		String password = "password";
		int minStrengthMillis = 100;
		long startTime = System.currentTimeMillis();
		String hashed = hashPassword(password,minStrengthMillis);
		long elapsedTime = System.currentTimeMillis() - startTime;
		LibTest.asrt( elapsedTime >= minStrengthMillis );
		LibTest.asrt( verifyPassword(password,hashed) );
        //System.out.println( "number of iterations: "+ Long.parseLong(hashed.split(":")[0],36) );
		return true;
	}



    public static String uniqID() {
        return uniqID(20);
    }
    public static String uniqID( int len ) {
        return uniqID("U",len);
    }
    public static String uniqID( String prefix ) {
        return uniqID(prefix,-1);
    }
    public static String uniqID( String prefix, int suffixLen ) {
        if (suffixLen<0) suffixLen=8;
        int totalLen = prefix.length() + suffixLen;
        StringBuilder buf = new StringBuilder();
        buf.append( Lib.isEmpty(prefix) ? "U" : prefix );
        int charsToFill = totalLen - buf.length();
        int STAMP_LEN = "YYYYMMDDHHMMSS".length(); // not every possible milli-value is possible on some platforms
        if ( charsToFill > 4+STAMP_LEN ) {
            String timeStamp = LibString.onlyAlphaNum( timeStamp() );
            if ( timeStamp.length() > STAMP_LEN ) timeStamp = timeStamp.substring(0,STAMP_LEN);
            buf.append(timeStamp);
        }
        String counterStr = toBase62( _uniqID.incrementAndGet() );
        charsToFill = totalLen - buf.length();
        if ( charsToFill >= counterStr.length() ) {
            buf.append( counterStr );
        } else {
            buf.append( counterStr.substring(0,charsToFill) );
        }
        charsToFill = totalLen - buf.length();
        while (charsToFill>0) {
            String rand = toBase62( new Random().nextLong() );
            buf.append(rand.substring( 0, Math.min(rand.length(),charsToFill) ));
            charsToFill = totalLen - buf.length();
        }
        return buf.toString();
    }
    public static AtomicLong _uniqID = new AtomicLong(0);
    @SuppressWarnings("unused")
    private static boolean uniqID_TEST_() {
        // generate 100 with same prefix, and make sure they are unique
        String prefix = "P";
        int len = 5;
        Set<String> ids = new HashSet<>();
        for (int i=0; i<100; i++) {
            String id = uniqID(prefix,len);
            LibTest.asrtEQ( id.length(), len+1, "length wrong" );
            LibTest.asrt(! ids.contains(id), "not unique!" );
            ids.add(id);
        }
        return true;
    }



    public static String toBase62(long value) {
        // good for IDs
        value = Math.abs(value);
        StringBuilder sb = new StringBuilder();
        final String DIGITS = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        do {
            int i = (int)(value % 62);
            sb.append( DIGITS.charAt(i) );
            value /= 62;
        } while (value>0);
        return sb.reverse().toString();
    }
    @SuppressWarnings("unused")
    private static boolean toBase62_TEST_( boolean findLineNumber ) {
        LibTest.asrtEQ( toBase62(0), "0" );
        LibTest.asrtEQ( toBase62(1), "1" );
        LibTest.asrtEQ( toBase62(61), "z" );
        LibTest.asrtEQ( toBase62(62), "10" );
        return true;
    }



    /**
     * A wrapper for java's URL to take the pain out.
     * (e.g. having to catch exceptions, having to construct with URI's, and having to figure out default ports)
     * No need to use this class for anything except construction, e.g.:
     *    new Lib.Url("http://www.google.com/").url // no mandatory try/catch block needed
     * or
     *    new Lib.Url("https://www.google.com/").port;
     *    // ^ returns 443, even though java.net.URL's getPort() would return -1
    **/
    public static class Url {
        public final java.net.URL url;
        public final String protocol;
        public final String host;
        public final int port;
        public final String path;
        public final String query;
        public final String fragment;
        public final boolean isHttps;
        public Url( String urlStr ) {
            try {
                url = new URI(urlStr).toURL();
                protocol = url.getProtocol();
                host = url.getHost();
                int p = url.getPort();
                path = url.getPath();
                query = url.getQuery();
                fragment = url.getRef();
                isHttps = "https".equals( protocol.toLowerCase() );
                port = p<0 ? ( isHttps ? 443 : 80 ) : p;
            } catch (Throwable t) {
                if (t instanceof RuntimeException) throw (RuntimeException)t;
                throw new RuntimeException(t);
            }
        }
    }



    /**
    * a better Map.of; which has ordered keys, allows nulls, is immutable, and toString is json.
    **/
    public static <K,V> Map<K,V> mapOf( Object... keyVals ) {
        Map<K,V> map = new LinkedHashMap<>();
        for (int i=0; i<keyVals.length; i+=2) {
            @SuppressWarnings("unchecked")
            K k = (K) keyVals[i];
            if ( keyVals.length == (i+1) ) {
                if (k==null) break; // ok if null key has no value
                throw new IllegalArgumentException("last key -- "+ k +" -- has no value");
            }
            @SuppressWarnings("unchecked")
            V v = (V) keyVals[i+1];
            map.put(k,v);
        }
        return new AbstractMap<K,V>() {
            public Set< Entry<K,V> > entrySet() {
                return map.entrySet();
            }
            public String toString() {
                return JsonEncoder.encode(map);
            }
        };
    }
    @SuppressWarnings("unused")
    private static boolean mapOf_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        Map<String,Integer> map = mapOf( "a",1, "b",2, "c",3, null,null );
        String[] keys = map.keySet().toArray(new String[0]);
        Integer[] vals = map.values().toArray(new Integer[0]);
        List<String> expectedKeys = listOf("a","b","c",null);
        List<Integer> expectedVals = listOf(1,2,3,null);
        LibTest.asrtEQ(keys,expectedKeys);
        LibTest.asrtEQ(vals,expectedVals);
        String expectedStr = "{\"a\":1,\"b\":2,\"c\":3,null:null}";
        LibTest.asrtEQ( map.toString(), expectedStr );
        // odd number of args ok as long as last one is null
        map = mapOf( "a",1, "b",2, "c",3, null );
        LibTest.asrtEQ( map.size(), 3 );
        try {
            map = mapOf( "a",1, "b",2, "c",3, "d" );
            throw new RuntimeException("should have thrown IllegalArgumentException");
        } catch ( IllegalArgumentException e ) {} // expected
        return true;
    }
    /**
    * a better List.of; which allows nulls, is immutable, and toString is json.
    **/
    @SafeVarargs
    public static <T> List<T> listOf( T... vals ) {
        ArrayList<Object> list = new ArrayList<>( vals.length );
        for ( Object val: vals ) list.add(val);
        return new AbstractList<T>() {
            @SuppressWarnings("unchecked")
            public T get(int index) { return (T)list.get(index); }
            public int size() { return list.size(); }
            public String toString() { return JsonEncoder.encode(list); }
        };
    }
    @SuppressWarnings("unused")
    private static boolean listOf_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        List<Integer> list = listOf( 1,2,3,null );
        Integer[] vals = list.toArray(new Integer[0]);
        Integer[] expectedVals = {1,2,3,null};
        LibTest.asrtEQ(vals,expectedVals);
        String expectedStr = "[1,2,3,null]";
        LibTest.asrtEQ( list.toString(), expectedStr );
        return true;
    }



    /**
     * If micros are not specified, guarantees that the same stamp is not returned twice.
    **/
    public static String timeStamp( Long microsSinceEpoch, Boolean utc, String format ) {
        if ( microsSinceEpoch==null ) microsSinceEpoch = currentTimeMicros();
        if (utc==null) utc=Boolean.TRUE;
        if (format==null) format = "yyyy-MM-dd'_'HH-mm-ss-SSSSSSSSS";
        Instant now = Instant.ofEpochSecond( microsSinceEpoch/1000000, (microsSinceEpoch%1000000)*1000 );
        ZonedDateTime zonedNow = utc
            ? ZonedDateTime.ofInstant(now, ZoneId.of("UTC"))
            : ZonedDateTime.ofInstant(now, ZoneId.systemDefault())
        ;
        String stamp = zonedNow.format( DateTimeFormatter.ofPattern(format) );
        return stamp;
    }
    public static String timeStamp() { return timeStamp( null, null, null ); }
    public static String timeStamp( String format ) { return timeStamp( null, null, format ); }
    public static String timeStamp( Long microsSinceEpoch ) {
        return timeStamp( microsSinceEpoch, null, null );
    }
    @SuppressWarnings("unused")
    private static boolean timeStamp_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        String stamp = timeStamp();
        LibTest.asrt( stamp.matches( "\\d{4}-\\d{2}-\\d{2}_\\d{2}-\\d{2}-\\d{2}-\\d{9}" ) );
        for (int i=0; i<10; i++) {
            String nextStamp = timeStamp();
            LibTest.asrt( nextStamp.compareTo(stamp) > 0 );
            stamp = nextStamp;
        }
        return true;
    }



    /**
     * Returns the current time in microseconds since the epoch (1970-01-01T00:00:00Z).
     * The returned value is always greater than the last returned value.
     **/
    public static long currentTimeMicros() {
        long millis = System.currentTimeMillis();
        long nanos = System.nanoTime();
        long micros = millis * 1000 + (nanos % 1000000) / 1000;
        synchronized (_currentTimeMicros_lastReturned) {
            long lastReturned = _currentTimeMicros_lastReturned.get();
            if (micros <= lastReturned) {
                micros = lastReturned + 1;
            }
            _currentTimeMicros_lastReturned.set(micros);
            return micros;
        }
    }
    public static final AtomicLong _currentTimeMicros_lastReturned = new AtomicLong(0);
    @SuppressWarnings("unused")
    private static boolean currentTimeMicros_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        long lastTime = currentTimeMicros();
        for (int i=0; i<10; i++) {
            long thisTime = currentTimeMicros();
            if ( lastTime >= thisTime ) throw new RuntimeException("Time is not increasing!");
            lastTime = thisTime;
        }
        { // make sure currentTimeMicros is close to System.currentTimeMillis() * 1000
            long sysTime = System.currentTimeMillis() * 1000;
            long libTime = currentTimeMicros();
            final int MAX_DIFF_MICROS = 3*1000*1000;
            long diff = Math.abs(sysTime-libTime);
            LibTest.asrt( diff<MAX_DIFF_MICROS, "time diff:"+(sysTime-libTime)+" micros" );
        }
        return true;
    }



    /**
     * Checks if an object is empty.
     */
    public static boolean isEmpty( Object o ) {
        if ( o==null ) return true;
        if ( o instanceof CharSequence s ) {
            if ( s.length()==0 ) return true;
            for ( int i=0, len=s.length(); i<len; i++ ) if ( s.charAt(i)>' ' ) return false;
            return true;
        }
        if ( o instanceof Number n ) return n.doubleValue()<.0001 && n.doubleValue()>(-.9999);
        if ( o instanceof Collection<?> c ) return c.isEmpty();
        if ( o instanceof Map<?,?> m ) return m.isEmpty();
        if ( o.getClass().isArray() ) return Array.getLength(o)==0;
        return false;
    }
    @SuppressWarnings({"unused"})
    private static boolean isEmpty_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        LibTest.asrt( isEmpty(null), "null should be empty" );
        LibTest.asrt( isEmpty(""), "empty string should be empty" );
        LibTest.asrt( isEmpty(" \t\n\r"), "whitespace should be empty" );
        LibTest.asrt( !isEmpty("x"), "non-empty string should not be empty" );
        LibTest.asrt( isEmpty(0), "zero should be empty" );
        LibTest.asrt( !isEmpty(1), "non-zero should not be empty" );
        LibTest.asrt( isEmpty(new ArrayList<>()), "empty list should be empty" );
        LibTest.asrt( !isEmpty(List.of(1)), "non-empty list should not be empty" );
        LibTest.asrt( isEmpty(new HashMap<>()), "empty map should be empty" );
        LibTest.asrt( !isEmpty(Map.of(1,2)), "non-empty map should not be empty" );
        LibTest.asrt( isEmpty(new int[0]), "empty array should be empty" );
        LibTest.asrt( !isEmpty(new int[]{1}), "non-empty array should not be empty" );
        return true;
    }



    public static boolean notEmpty( Object o ) { return !isEmpty(o); }
    @SuppressWarnings({"unused"})
    private static boolean notEmpty_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        LibTest.asrt( !notEmpty(null), "null should not be not-empty" );
        LibTest.asrt( !notEmpty(""), "empty string should not be not-empty" );
        LibTest.asrt( !notEmpty(" \t\n\r"), "whitespace should not be not-empty" );
        LibTest.asrt( notEmpty("x"), "non-empty string should be not-empty" );
        return true;
    }



    public static String nvl( Object o, String def ) { return o==null ? def : o.toString(); }
    public static <T> T nvl( T o, T def ) { return o==null ? def : o; }
    @SuppressWarnings({"unused"})
    private static boolean nvl_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        LibTest.asrtEQ( "default", nvl(null, "default"), "null should return default" );
        LibTest.asrtEQ( "value", nvl("value", "default"), "non-null should return value" );
        Integer i = 42;
        LibTest.asrtEQ( i, nvl(i, 0), "non-null should return value" );
        LibTest.asrtEQ( 0, nvl((Integer)null, 0), "null should return default" );
        return true;
    }



    /**
     * Evaluates if an object should be considered "true"
     */
    @SuppressWarnings({"rawtypes"})
    public static boolean isTrue( Object o ) {
        if ( o==null ) return false;
        if ( o instanceof Number ) return ((Number)o).doubleValue()>.1 || ((Number)o).doubleValue()<(-.1);
        if ( o instanceof Boolean ) return ((Boolean)o).booleanValue();
        if ( o instanceof List ) return ((List)o).size()>0;
        if ( o instanceof Map ) return ((Map)o).size()>0;
        if ( o.getClass().isArray() ) return Array.getLength(o)>0;

        String s = o.toString().trim().toUpperCase();
        if ( s.length()==0 ) return false;
        if ( s.equals("TRUE") || s.equals("T") ) return true;
        if ( s.equals("FALSE") || s.equals("F") ) return false;
        if ( s.equals("ON") || s.equals("YES") || s.equals("Y") ) return true;
        if ( s.equals("OFF") || s.equals("NO") || s.equals("N") ) return false;
        if ( s.equals("NONE") ) return false;
        if ( s.equals("1") || s.equals("-1") ) return true;
        if ( s.matches("(0|\\s)*") ) return false;
        return true;
    }
    @SuppressWarnings({"unused"})
    private static boolean isTrue_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        LibTest.asrt( !isTrue(null), "null should not be true" );
        LibTest.asrt( isTrue(true), "true should be true" );
        LibTest.asrt( !isTrue(false), "false should not be true" );
        LibTest.asrt( isTrue(1), "1 should be true" );
        LibTest.asrt( !isTrue(0), "0 should not be true" );
        LibTest.asrt( isTrue("true"), "\"true\" should be true" );
        LibTest.asrt( isTrue("TRUE"), "\"TRUE\" should be true" );
        LibTest.asrt( isTrue("t"), "\"t\" should be true" );
        LibTest.asrt( isTrue("yes"), "\"yes\" should be true" );
        LibTest.asrt( isTrue("y"), "\"y\" should be true" );
        LibTest.asrt( isTrue("on"), "\"on\" should be true" );
        LibTest.asrt( !isTrue("false"), "\"false\" should not be true" );
        LibTest.asrt( !isTrue("FALSE"), "\"FALSE\" should not be true" );
        LibTest.asrt( !isTrue("f"), "\"f\" should not be true" );
        LibTest.asrt( !isTrue("no"), "\"no\" should not be true" );
        LibTest.asrt( !isTrue("n"), "\"n\" should not be true" );
        LibTest.asrt( !isTrue("off"), "\"off\" should not be true" );
        LibTest.asrt( !isTrue("none"), "\"none\" should not be true" );
        LibTest.asrt( isTrue("1"), "\"1\" should be true" );
        LibTest.asrt( isTrue("-1"), "\"-1\" should be true" );
        LibTest.asrt( !isTrue("0"), "\"0\" should not be true" );
        LibTest.asrt( isTrue(List.of(1)), "non-empty list should be true" );
        LibTest.asrt( !isTrue(new ArrayList<>()), "empty list should not be true" );
        return true;
    }



    /**
     * Wraps an Iterator with an Iterable.
     */
    public static <T> Iterable<T> iterable( Iterator<T> it ) {
        final AtomicBoolean alreadyUsed = new AtomicBoolean(false);
        return new Iterable<T>() {
            public Iterator<T> iterator() {
                if ( alreadyUsed.getAndSet(true) ) throw new IllegalStateException("iterator() already called");
                return it;
            }
        };
    }
    @SuppressWarnings({"unused"})
    private static boolean iterable_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        Iterator<Integer> it = List.of(1, 2, 3).iterator();
        Iterable<Integer> itbl = iterable(it);
        List<Integer> result = new ArrayList<>();
        for ( Integer i:itbl ) result.add(i);
        LibTest.asrtEQ( List.of(1, 2, 3), result, "iterable should iterate over all elements" );
        try {
            for ( Integer i:itbl ) { System.err.println("Should not be able to reach here"); return false; }
        } catch ( Exception expected ) {}
        return true;
    }



    /*
     * Returns an immutable list version of whatever is passed to it.
    */
    @SuppressWarnings({"rawtypes","unchecked"})
    public static List asList( Object o ) {
        if (o==null) return Collections.emptyList();
        if (o instanceof List) return Collections.unmodifiableList( (List)o );
        if (o instanceof Iterable) return asList( ((Iterable)o).iterator() );
        if (o instanceof Iterator) {
            List<Object> list = new ArrayList<>();
            Iterator it = (Iterator)o;
            while (it.hasNext()) list.add(it.next());
            return Collections.unmodifiableList(list);
        }
        if ( o.getClass().isArray() ) {
            return new AbstractList() {
                private final int length = Array.getLength(o);
                public int size() { return length; }
                public Object get(int index) { return Array.get(o, index); }
            };
        }
        if ( o instanceof Stream<?> ) return ((Stream<?>)o).collect( Collectors.toList() );
        return Collections.singletonList(o);
    }
    @SuppressWarnings("unused")
    private static boolean asList_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        List<?> l = asList( List.of(1,2,3).iterator() );
        LibTest.asrtEQ( 3, l.size() );
        LibTest.asrtEQ( 1, l.get(0) );
        LibTest.asrtEQ( 2, l.get(1) );
        LibTest.asrtEQ( 3, l.get(2) );
        return true;
    }



    /**
    * Compares without regard to case or difference in space or type;
    * e.g. "123" matches 123, " testing \t 123 " matches "TEStING 123"
    **/
    @SuppressWarnings({"unchecked"})
    public static <T> T findFirst( Collection<T> list, T... items ) {
        for ( T item : items ) {
            for ( T listitem : list ) {
                if ( isEqual(item,listitem) ) return item;
            }
        }
        return null;
    }
    @SuppressWarnings({"unchecked"})
    public static <T> T findValue( T value, Collection<T> values ) {
        return findFirst(values,value);
    }
    /**
    * Returns quickly if the key exists in the map.
    * If the key does not exist in the map, searches for a key that LOOKS LIKE the given key,
    * without regard to case or difference in space or type;
    * e.g. "123" matches 123, " testing \t 123 " matches "TEStING 123"
    **/
    @SuppressWarnings({"unchecked"})
    public static <K> K findKey( Map<K,?> map, K... keys2find ) {
        for ( K key2find : keys2find ) {
            if ( map==null || key2find==null ) return null;
            if ( map.containsKey(key2find) ) return key2find;
        }
        for (Iterator<K> it=map.keySet().iterator(); it.hasNext();) {
            K tryK = it.next();
            for ( K key2find : keys2find ) {
                if ( isEqual(tryK,key2find) ) return tryK;
            }
        }
        return null;
    }
    @SuppressWarnings("unchecked")
    public static <K> K findKey( K key, Map<K,?> map ) {
        return findKey(map,key);
    }
    @SuppressWarnings("unused")
    private static boolean findKey_TEST_() {
        HashMap<Object,Object> map = new HashMap<>();
        map.put( 123, "456" );
        LibTest.asrtEQ( findKey(" 123 ",map), (Object)123 );
        map.put( " testing \t 123 ", "OK" );
        LibTest.asrtEQ( findKey("TESTING 123",map), " testing \t 123 " );
        LibTest.asrtEQ( findValue(" ok \r\n",map.values()), "OK" );
        LibTest.asrtEQ( null, findKey(88.8,map) );
        return true;
    }



    public static boolean isEqual( Object a, Object b ) {
        if ( a==null && b==null ) return true;
        if ( a==null || b==null ) return false;
        if ( a instanceof Number && b instanceof Number ) {
            return (
                ((Number)a).doubleValue() == ((Number)b).doubleValue()
            ) || (
                a.toString().equals( b.toString() )
                // e.g. NaN fails == test. This is hard-coded into the JVM, not the java.lang.Double class.
            );
        }
        if ( (a instanceof byte[] aArr) && (b instanceof byte[] bArr) ) {
            if ( aArr.length != bArr.length ) return false;
            for ( int i=0; i<aArr.length; i++ ) if ( aArr[i] != bArr[i] ) return false;
            return true;
        }
        if ( (a instanceof Map<?,?> aMap) && (b instanceof Map<?,?> bMap) ) {
            if ( aMap.size() != bMap.size() ) return false;
            for ( Object k : aMap.keySet() ) {
                @SuppressWarnings("unchecked")
                Map<Object,Object> bm = (Map<Object,Object>)b;
                Object bKey = findKey(k,bm);
                if ( k!=null && bKey==null ) return false;
                if (! isEqual(aMap.get(k),bMap.get(bKey)) ) return false;
            }
            return true;
        }
        { // try making sets and comparing them
            @SuppressWarnings("unchecked")
            Set<Object> aSet = new LinkedHashSet<Object>( asList(a) );
            @SuppressWarnings("unchecked")
            Set<Object> bSet = new LinkedHashSet<Object>( asList(b) );
            if ( aSet.size() != bSet.size() ) return false;
            if (!( aSet.contains(a) && bSet.contains(b) )) {
                // looks like one or both of them were unpacked
                for ( Object aObj : aSet ) {
                    if ( bSet.contains(aObj) ) continue;
                    return false;
                }
                return true;
            }
        }
        if ( a.getClass().getName().compareTo(b.getClass().getName()) > 0 ) {
            // swap so we can know the order of the types
            Object tmp=a; a=b; b=tmp;
        }
        if ( (a instanceof Character c) && (b instanceof Number n) ) {
            int cInt = (int)c;
            int nInt = n.intValue();
            return cInt==nInt;
        }
        return LibString.normalSpace( a.toString() ).equalsIgnoreCase(LibString.normalSpace( b.toString() ));
    }
    @SuppressWarnings("unused")
    private static boolean isEqual_TEST_() {
        LibTest.asrt( isEqual(null,null), "isEqual test 1 failed" );
        LibTest.asrt( !isEqual(null,""), "isEqual test 2 failed" );
        LibTest.asrt( isEqual("oK","Ok"), "isEqual test 3 failed" );
        LibTest.asrt( isEqual(1,"1"), "isEqual test 4 failed" );
        return true;
    }



    /**
     * Gets the class name at a specific frame in the stack trace
     */
    public static String getClassName( int backupFrames ) {
        return new Throwable().getStackTrace()[backupFrames].getClassName();
    }
    @SuppressWarnings({"unused"})
    private static boolean getClassName_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        LibTest.asrtEQ( Lib.class.getName(), getClassName(0), "getClassName(0) should return this class" );
        return true;
    }



    /**
     * Gets the current class
     */
    @SuppressWarnings({"rawtypes"})
    public static Class thisClass() {
        try { return Class.forName(getClassName(2)); }
        catch ( ClassNotFoundException e ) { throw new RuntimeException(e); }
    }
    @SuppressWarnings({"unused"})
    private static boolean thisClass_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        LibTest.asrt( thisClass().getName().endsWith("Lib"), "thisClass() should return Lib class" );
        return true;
    }



    public static Class<?> getCallingClass() {
        try {
            return Class.forName( getCallingClassName() );
        } catch ( ClassNotFoundException cnfe ) {
            throw new RuntimeException(cnfe);
        }
    }
    public static String getCallingClassName() {
        StackTraceElement[] stak = new Throwable().getStackTrace();
        String thisClassName = stak[0].getClassName();
        String libTestClassName = LibTest.class.getName();
        // skip all frames from this class and LibTest
        for (int i=0; i<stak.length; i++) {
            String className = stak[i].getClassName();
            if ( className.equals(thisClassName) ) continue;
            if ( className.equals(libTestClassName) ) continue;
            thisClassName = className;
            break;
        }
        return thisClassName;
    }



    /**
    * Shows a dialog and ignores the user's response.
    *
    * Any program that uses this method can set the default look and feel using
    * something like this:
    * UIManager.setLookAndFeel( UIManager.getSystemLookAndFeelClassName() );
    **/
    public static void dialog( String message, boolean isError, int closeAfterSeconds ) {
        String title = isError ? "ERROR" : "Info";
        Object[] options = isError ? new Object[]{"Abort"} : new Object[]{"OK"};
        boolean isInfo = !isError;
        dialog(title,message,options,0,isInfo,false,false,isError,null,closeAfterSeconds);
    }
    /**
     * Shows a dialog, and returns the user's response.
     *
     * Any program that uses this method can set the default look and feel using
     * something like this: UIManager.setLookAndFeel(
     * UIManager.getSystemLookAndFeelClassName() );
     **/
    public static Object dialog(String title, String message, Object[] options,
        int defaultOptionIndex, boolean isInfo, boolean isQuestion,
        boolean isWarn, boolean isError, javax.swing.JFrame optionalFrame,
        int closeAfterSeconds
    ) {
        if (closeAfterSeconds<=0) closeAfterSeconds = 24*60*60; // 1 day
        final javax.swing.JFrame frame = (
            optionalFrame == null ?
            new javax.swing.JFrame( "dialog@" + System.currentTimeMillis() ) : optionalFrame
        );
        frame.setDefaultCloseOperation(javax.swing.JFrame.DISPOSE_ON_CLOSE);
        Object result = null;
        try {
            int msgType = javax.swing.JOptionPane.PLAIN_MESSAGE;
            if (isError) {
                msgType = javax.swing.JOptionPane.ERROR_MESSAGE;
            } else if (isWarn) {
                msgType = javax.swing.JOptionPane.WARNING_MESSAGE;
            } else if (isQuestion) {
                msgType = javax.swing.JOptionPane.QUESTION_MESSAGE;
            } else if (isInfo) {
                msgType = javax.swing.JOptionPane.INFORMATION_MESSAGE;
            }
            Object defaultOption = null;
            if (options != null && defaultOptionIndex >= 0
                    && defaultOptionIndex < options.length) {
                defaultOption = options[defaultOptionIndex];
            }
            final long[] stopAfter = new long[]{ System.currentTimeMillis() + closeAfterSeconds*1000 };
            Thread timeoutCounter = new Thread() {
                public void run() {
                    while (true) {
                        try { Thread.sleep(500); }catch(InterruptedException ie){ break; }
                        synchronized( stopAfter ){
                            if ( System.currentTimeMillis() >= stopAfter[0] ) break;
                        }
                    }
                    frame.dispose();
                }
            };
            timeoutCounter.setDaemon(false);
            timeoutCounter.start();
            int selIdx = javax.swing.JOptionPane.showOptionDialog(
                frame, message, title, javax.swing.JOptionPane.YES_NO_CANCEL_OPTION, msgType,
                null, options, defaultOption
            );
            synchronized(stopAfter){ stopAfter[0] = -1; }
            if (selIdx>=0 && options!=null && selIdx<=options.length) {
                result = options[selIdx];
            }
        } finally {
            try{ frame.dispose(); } catch ( Throwable ignore ) {}
        }
        return result;
    }
    /**
     * Shows a dialog, and returns the user's response.
     *
     * Any program that uses this method can set the default look and feel using
     * something like this: UIManager.setLookAndFeel(
     * UIManager.getSystemLookAndFeelClassName() );
     * @return the user's response, or null if the dialog was cancelled, closed, or timed out.
     **/
    public static String dialog(
        String title, String message, String defaultValue, javax.swing.JFrame optionalFrame, int closeAfterSeconds
    ) {
        if (closeAfterSeconds <= 0) closeAfterSeconds = 24 * 60 * 60; // 1 day
        final javax.swing.JFrame frame = (
            optionalFrame == null ?
            new javax.swing.JFrame("dialog@" + System.currentTimeMillis()) : optionalFrame
        );
        frame.setDefaultCloseOperation(javax.swing.JFrame.DISPOSE_ON_CLOSE);
        String userResponse = null;
        try {
            final long[] stopAfter = new long[]{System.currentTimeMillis() + closeAfterSeconds * 1000};
            Thread timeoutCounter = new Thread() {
                public void run() {
                    while (true) {
                        try {
                            Thread.sleep(500);
                        } catch (InterruptedException ie) {
                            break;
                        }
                        synchronized (stopAfter) {
                            if (System.currentTimeMillis() >= stopAfter[0]) break;
                        }
                    }
                    frame.dispose();
                }
            };
            timeoutCounter.setDaemon(true);
            timeoutCounter.start();
            javax.swing.JPanel panel = new javax.swing.JPanel(new java.awt.BorderLayout());
            panel.add(new javax.swing.JLabel(message), java.awt.BorderLayout.CENTER);
            javax.swing.JTextField textField = new javax.swing.JTextField(defaultValue);
            panel.add(textField, java.awt.BorderLayout.SOUTH);
            String[] options = {"OK","Cancel"};
            int result = javax.swing.JOptionPane.showOptionDialog(
                frame, panel, title, javax.swing.JOptionPane.DEFAULT_OPTION, javax.swing.JOptionPane.QUESTION_MESSAGE,
                null, options, options[0]
            );
            synchronized (stopAfter) { stopAfter[0] = -1; }
            if (result == 0) userResponse = textField.getText();
        } finally {
            try { frame.dispose(); } catch (Throwable ignore) {}
        }
        return userResponse;
    }
    // Commented out for headless environments
    // @SuppressWarnings("unused")
    // private static boolean dialog_TEST_() {
    //     String userResponse = dialog( "Test", "Enter something:", "ok", null, 1 );
    //     System.out.println( "userResponse: " + userResponse );
    //     return true;
    // }



    public static Process osCmd( List<String> cmd, Map<String,String> envChanges, File cwd ) throws IOException {
        ProcessBuilder processBuilder = new ProcessBuilder(cmd);
        Map<String,String> environment = processBuilder.environment();
        if (envChanges==null) envChanges = Collections.emptyMap();
        for ( Map.Entry<String,String> entry : envChanges.entrySet() ) {
            environment.put(entry.getKey(), entry.getValue());
        }
        if (cwd!=null) processBuilder.directory(cwd);
        return processBuilder.start();
    }



    /**
     * Pushes and pulls bytes into and out of a Process.
     * @return The Process finish status code.
     */
    public static int OSProcIO(
        Process process, InputStream stdin,
        OutputStream stdout, OutputStream stderr
    ) {
        final Process proc = process;
        final InputStream finp = stdin;
        final OutputStream fout = stdout;
        final OutputStream ferr = stderr;
        final StringBuffer extraErrors = new StringBuffer();
        Thread inpThread = new Thread() {
            public void run() {
                try {
                    OutputStream out = proc.getOutputStream();
                    byte[] buf = new byte[1024];
                    while (true) {
                        int bytCount = finp==null ? -1 : finp.read(buf);
                        if (bytCount<0) break;
                        out.write(buf,0,bytCount);
                    }
                    out.close();
                } catch (Throwable e) {
                    synchronized (extraErrors) {
                        extraErrors.append( e.toString() ).append("\n");
                    }
                }
            }
        };
        Thread outThread = new Thread() {
            public void run() {
                try {
                    InputStream inp = proc.getInputStream();
                    byte[] buf = new byte[1024];
                    while (true) {
                        int bytCount = inp.read(buf);
                        if (bytCount<0) break;
                        if (fout!=null) fout.write(buf,0,bytCount);
                    }
                    inp.close();
                } catch (Throwable e) {
                    synchronized (extraErrors) {
                        extraErrors.append( e.toString() ).append("\n");
                    }
                }
            }
        };
        Thread errThread = new Thread() {
            public void run() {
                try {
                    InputStream inp = proc.getErrorStream();
                    byte[] buf = new byte[1024];
                    while (true) {
                        int bytCount = inp.read(buf);
                        if (bytCount<0) break;
                        if (ferr!=null) ferr.write(buf,0,bytCount);
                    }
                    inp.close();
                } catch (Throwable e) {
                    synchronized (extraErrors) {
                        extraErrors.append( e.toString() ).append("\n");
                    }
                }
            }
        };
        inpThread.start();
        outThread.start();
        errThread.start();
        try {
            inpThread.join();
            outThread.join();
            errThread.join();
            int result = proc.waitFor();
            synchronized (extraErrors) {
                if (ferr!=null && extraErrors.length() > 0) ferr.write(extraErrors.toString().getBytes());
            }
            return result;
        } catch (InterruptedException ie) {
            throw new RuntimeException(ie);
        } catch (IOException io) {
            throw new RuntimeException(io);
        }
    }



    public static void main( String[] args ) throws Exception { LibTest.testClass(); }
}
