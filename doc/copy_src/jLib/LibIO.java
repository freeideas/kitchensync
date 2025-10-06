package jLib;
import java.io.*;
import java.nio.*;
import java.nio.charset.*;
import java.util.*;


public class LibIO {

    public static char[] readFully( Reader r ) {
        try {
            CharArrayWriter w = new CharArrayWriter();
            copy(r,w);
            return w.toCharArray();
        } catch ( IOException e ) {
            throw new RuntimeException("readFully(Reader) failed",e);
        }
    }
    public static byte[] readFully( InputStream inp ) {
        try {
            ByteArrayOutputStream baos = new ByteArrayOutputStream();
            copy(inp,baos);
            return baos.toByteArray();
        } catch ( IOException e ) {
            throw new RuntimeException("readFully(InputStream) failed",e);
        }
    }


    public static long copy( Reader inp, Writer... outs )
        throws IOException {
        long totalWritten = 0;
        char[] buf = new char[8192];
        while (true) {
            int readCount = inp.read(buf);
            if ( readCount<0 ) break;
            if ( readCount>0 ) {
                for ( Writer out : outs ) {
                    out.write(buf,0,readCount);
                }
                totalWritten += readCount;
            }
        }
        return totalWritten;
    }
    public static long copy( InputStream inp, OutputStream... outs )
        throws IOException {
        long totalWritten = 0;
        byte[] buf = new byte[8192];
        while (true) {
            int readCount = inp.read(buf);
            if ( readCount<0 ) break;
            if ( readCount>0 ) {
                for ( OutputStream out : outs ) {
                    out.write(buf,0,readCount);
                }
                totalWritten += readCount;
            }
        }
        return totalWritten;
    }
    @SuppressWarnings("unused")
    private static boolean copy_slow_TEST_() throws IOException {
        final int TEST_SIZE = 1000000;
        { // Reader/Writer
            StringBuilder sb = new StringBuilder();
            for (int i=0; i<TEST_SIZE; i++) {
                sb.append((char)('A' + i % 26));
            }
            String testData = sb.toString();
            StringReader r = new StringReader(testData);
            StringWriter w = new StringWriter();
            long copiedChars = copy(r, w);
            LibTest.asrtEQ((long)TEST_SIZE, copiedChars);
            LibTest.asrtEQ(testData, w.toString());
        }
        { // InputStream/OutputStream
            byte[] testData = new byte[TEST_SIZE];
            for (int i=0; i<TEST_SIZE; i++) {
                testData[i] = (byte)('A' + i % 26);
            }
            ByteArrayInputStream inp = new ByteArrayInputStream(testData);
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            long copiedBytes = copy(inp, out);
            LibTest.asrtEQ((long)TEST_SIZE, copiedBytes);
            LibTest.asrtEQ(Arrays.toString(testData), Arrays.toString(out.toByteArray()));
        }
        return true;
    }


    public static InputStream toInputStream( Object o ) throws IOException {
        if (o==null) return new ByteArrayInputStream( new byte[0] );
        if ( o instanceof InputStream inp ) return inp;
        if ( o instanceof Reader r ) return readerInputStream(r);
        if ( o instanceof byte[] bArr ) return new ByteArrayInputStream(bArr);
        if ( o instanceof ByteBuffer bb ) return new ByteArrayInputStream( bb.array() );
        if ( o instanceof CharSequence cs ) return new ByteArrayInputStream( cs.toString().getBytes(StandardCharsets.UTF_8) );
        if ( o instanceof File f ) return new FileInputStream(f);
        throw new IOException("toInputStream: can't convert " + o.getClass().getName());
    }


    public static byte[] toBytes( InputStream inp, boolean close ) throws IOException {
        ByteArrayOutputStream baos = new ByteArrayOutputStream();
        copy(inp,baos);
        if (close) inp.close();
        return baos.toByteArray();
    }


    public static boolean isText( byte[] bArr ) {
        return !isBinary(bArr);
    }
    public static boolean isBinary( byte[] bArr ) {
        if (bArr==null || bArr.length==0) return false;
        int checkLen = Math.min(bArr.length, 512);
        for (int i=0; i<checkLen; i++) {
            int b = bArr[i] & 0xFF;
            if (b<32 && b!=9 && b!=10 && b!=13) return true;
        }
        return false;
    }


    public static Throwable append2file( File f, Object data ) {
        try ( FileOutputStream fout = new FileOutputStream(f,true) ) {
            if ( data instanceof InputStream inp ) {
                copy(inp,fout);
            } else if ( data instanceof Reader r ) {
                copy(r, new OutputStreamWriter(fout));
            } else if ( data instanceof byte[] bArr ) {
                fout.write(bArr);
            } else {
                fout.write( data.toString().getBytes() );
            }
            return null;
        } catch ( Throwable t ) {
            return t;
        }
    }
    @SuppressWarnings("unused")
    private static boolean append2file_TEST_() throws IOException {
        File tmpFile = File.createTempFile("test", ".txt");
        tmpFile.deleteOnExit();
        
        append2file(tmpFile, "Hello");
        append2file(tmpFile, " World");
        
        String content = new String(readFully(new FileInputStream(tmpFile)));
        LibTest.asrtEQ("Hello World", content);
        
        tmpFile.delete();
        return true;
    }


    public static InputStream multicast( InputStream inp, OutputStream... streams ) {
        return new MultiplexedInputStream(inp,streams);
    }
    public static OutputStream multicast( OutputStream... streams ) {
        return new MultiplexedOutputStream(streams);
    }
    @SuppressWarnings("unused")
    private static boolean multicast_TEST_() throws Exception {
        ByteArrayOutputStream baosA = new ByteArrayOutputStream();
        ByteArrayOutputStream baosB = new ByteArrayOutputStream();
        String s = "Hello world!";
        OutputStream os = multicast(baosA,baosB);
        os.write( s.getBytes() );
        os.flush();
        os.close();
        LibTest.asrtEQ( s, new String( baosA.toByteArray() ) );
        baosA.reset();
        baosB.reset();
        InputStream inp = multicast( new ByteArrayInputStream( s.getBytes() ), baosA, baosB );
        byte[] buf = new byte[1024];
        int len = inp.read(buf);
        LibTest.asrtEQ( s.length(), len );
        LibTest.asrtEQ( s, new String( baosB.toByteArray() ) );
        return true;
    }


    public static class MultiplexedOutputStream extends OutputStream {
        public final List<OutputStream> outs = Collections.synchronizedList( new LinkedList<>() );
        public MultiplexedOutputStream( OutputStream... outs ) {
            for ( OutputStream out : outs ) if(out!=null)this.outs.add(out);
        }
        @Override public void write( byte[] bArr, int off, int len ) {
            synchronized (outs) {
                for ( OutputStream out : outs ) {
                    try{ out.write(bArr,off,len); }catch(Throwable t){}
                }
            }
        }
        @Override public void write( int b ) {
            synchronized (outs) {
                for ( OutputStream out : outs ) {
                    try{ out.write(b); }catch(Throwable t){}
                }
            }
        }
        @Override public void flush() {
            synchronized (outs) {
                for ( OutputStream out : outs ) {
                    try{ out.flush(); }catch(Throwable t){}
                }
            }
        }
        @Override public void close() {
            synchronized (outs) {
                for ( OutputStream out : outs ) {
                    try{ out.close(); }catch(Throwable t){}
                }
                outs.clear();
            }
        }
    }


    public static class MultiplexedInputStream extends InputStream {
        InputStream inp = null;
        OutputStream[] outs = null;
        public MultiplexedInputStream( InputStream inp, OutputStream... outs ) {
            this.inp = inp;
            this.outs = outs;
        }
        @Override public int read() throws IOException {
            int b = inp.read();
            if ( b>=0 ) for ( OutputStream out : outs ) {
                try{ out.write(b); }catch(Throwable t){}
            }
            return b;
        }
        @Override public int read( byte b[], int off, int len ) throws IOException {
            int count = inp.read(b,off,len);
            if ( count>0 ) for ( OutputStream out : outs ) {
                try{ out.write(b,off,count); }catch(Throwable t){}
            }
            return count;
        }
        @Override public void close() throws IOException {
            inp.close();
        }
    }


    /**
     * Converts a Reader to an InputStream using UTF-8 encoding
     */
    public static InputStream readerInputStream( Reader r ) {
        return new ReaderInputStream(r);
    }
    @SuppressWarnings("unused")
    private static boolean readerInputStream_TEST_() throws IOException {
        String testStr = "Hello World! Special chars: åäö";
        StringReader reader = new StringReader(testStr);
        InputStream is = readerInputStream(reader);
        byte[] result = readFully(is);
        String resultStr = new String(result, StandardCharsets.UTF_8);
        LibTest.asrtEQ(testStr, resultStr);
        return true;
    }
    
    
    public static class ReaderInputStream extends InputStream {
        private final Reader reader;
        private final CharBuffer charBuffer = CharBuffer.allocate(1024);
        private final ByteBuffer byteBuffer = ByteBuffer.allocate(1024);
        private final CharsetEncoder encoder = StandardCharsets.UTF_8.newEncoder();
        private boolean endOfInput = false;

        public ReaderInputStream(Reader reader) {
            this.reader = reader;
            charBuffer.limit(0);
        }

        @Override
        public int read() throws IOException {
            byte[] singleByte = new byte[1];
            int result = read(singleByte, 0, 1);
            return result == -1 ? -1 : singleByte[0] & 0xFF;
        }

        @Override
        public int read(byte[] b, int off, int len) throws IOException {
            if (len == 0) return 0;
            
            int totalRead = 0;
            
            while (totalRead < len) {
                if (byteBuffer.hasRemaining()) {
                    int toRead = Math.min(len - totalRead, byteBuffer.remaining());
                    byteBuffer.get(b, off + totalRead, toRead);
                    totalRead += toRead;
                } else {
                    byteBuffer.clear();
                    CoderResult result = encoder.encode(charBuffer, byteBuffer, endOfInput);
                    
                    if (result.isUnderflow() && !endOfInput) {
                        charBuffer.compact();
                        int charsRead = reader.read(charBuffer);
                        if (charsRead == -1) {
                            endOfInput = true;
                        }
                        charBuffer.flip();
                    }
                    
                    byteBuffer.flip();
                    
                    if (!byteBuffer.hasRemaining() && endOfInput) {
                        return totalRead == 0 ? -1 : totalRead;
                    }
                }
            }
            
            return totalRead;
        }

        @Override
        public void close() throws IOException {
            reader.close();
        }
    }


    /**
     * Converts an OutputStream to a Writer using UTF-8 encoding
     */
    public static Writer writerOutputStream( OutputStream os ) {
        return new OutputStreamWriter(os, StandardCharsets.UTF_8);
    }
    @SuppressWarnings("unused")
    private static boolean writerOutputStream_TEST_() throws IOException {
        ByteArrayOutputStream baos = new ByteArrayOutputStream();
        Writer writer = writerOutputStream(baos);
        String testStr = "Hello World! Special chars: åäö";
        writer.write(testStr);
        writer.flush();
        String result = new String(baos.toByteArray(), StandardCharsets.UTF_8);
        LibTest.asrtEQ(testStr, result);
        return true;
    }


    public static void main( String[] args ) { LibTest.testClass(); }
}