package jLib;
import java.io.*;
import java.util.*;
import java.util.concurrent.atomic.*;

public class BlockingByteBuffer {

    private final CircularByteBuffer cbb = new CircularByteBuffer();
    private int maxBufSize;
    private boolean isClosing = false; // when closed, allows only reading
    public final Object lock = new Object();
    public final AtomicBoolean throwIOExceptionWhenFull = new AtomicBoolean(false);



    public BlockingByteBuffer( int maxBufSize ) {
        this.maxBufSize = maxBufSize;
    }
    public BlockingByteBuffer() {
        this(1024*1024);
    }



    public void setMaxBufSize( int maxBufSize ) {
        synchronized (lock) {
            this.maxBufSize = maxBufSize;
            lock.notifyAll();
        }
    }
    public int getMaxBufSize() {
        synchronized (lock) {
            return maxBufSize;
        }
    }



    public void reset() {
        synchronized (lock) {
            cbb.clear();
            isClosing = false;
            lock.notifyAll();
        }
    }



    public OutputStream getOutputStream() {
        final String caller = JsonEncoder.encode( Lib.getStackTrace(true) );
        return new OutputStream() {
            @Override public void write( int b ) throws IOException {
                write( new byte[]{(byte)b}, 0, 1 );
            }
            @Override public void write( byte[] arr ) throws IOException {
                write( arr, 0, arr.length );
            }
            @Override public void write( byte[] arr, int off, int len ) throws IOException {
                BlockingByteBuffer.this.write( arr, off, len );
            }
            @Override public void close() throws IOException {
                BlockingByteBuffer.this.close();
            }
            @Override public String toString() { return "bbout@"+caller; }
        };
    }



    public InputStream getInputStream() {
        final String caller = JsonEncoder.encode( Lib.getStackTrace(true) );
        return new InputStream() {
            @Override public int read() throws IOException {
                byte[] arr = new byte[1];
                int readCount = read( arr, 0, 1 );
                return readCount==1 ? ( arr[0] & 0xFF ) : -1;
            }
            @Override public int read( byte[] arr ) throws IOException {
                return BlockingByteBuffer.this.read( arr, 0, arr.length );
            }
            @Override public int read( byte[] arr, int off, int len ) throws IOException {
                return BlockingByteBuffer.this.read( arr, off, len );
            }
            @Override public int available() {
                return BlockingByteBuffer.this.size();
            }
            @Override public void close() {
                BlockingByteBuffer.this.close();
            }
            @Override public String toString() { return "bbinp@"+caller; }
        };
    }



    public boolean isClosed() {
        synchronized (lock) {
            boolean ret = isClosing && cbb.isEmpty();
            //L0g.log("isClosed()="+ret);
            return ret;
        }
    }



    public boolean isClosing() {
        synchronized (lock) {
            boolean ret = isClosing;
            //L0g.log("isClosing()="+ret);
            return ret;
        }
    }



    public void waitUntilClosed() {
        synchronized (lock) {
            while (! isClosed() ) {
                try { lock.wait(100); }
                catch ( InterruptedException interrupt ) { break; }
            }
        }
    }



    public boolean isFull() {
        synchronized (lock) {
            boolean ret = cbb!=null && cbb.size() >= maxBufSize;
            //L0g.log("isFull()="+ret);
            return ret;
        }
    }



    public boolean isEmpty() {
        synchronized (lock) {
            boolean ret = cbb!=null && cbb.isEmpty();
            //L0g.log("isEmpty()="+ret);
            return ret;
        }
    }



    public int size() {
        synchronized (lock) {
            int siz = cbb==null ? -1 : cbb.size();
            //L0g.log("size()="+siz);
            return siz;
        }
    }



    public void close() {
        synchronized (lock) {
            //L0g.log("bbb.close()");
            isClosing = true;
            lock.notifyAll();
        }
    }



    public void write( byte[] arr, int off, int len ) throws IOException {
        len = Math.min( len, arr.length-off );
        if (len<=0) return;
        while (len>0) {
        synchronized (lock) {
            //L0g.log( "write(arr,"+off+","+len+"):"+new String(arr,off,len) );
            while ( isFull() ) {
                if (throwIOExceptionWhenFull.get()) throw new IOException( "buffer full" );
                try { lock.wait(500); }
                catch ( InterruptedException interrupt ) { break; }
            }
            if (isClosing) throw new IOException( "Buffer closed." );
            int capacityRemaining = maxBufSize - cbb.size();
            int lenToWrite = Math.min( len, capacityRemaining );
            cbb.addLast( arr, off, lenToWrite );
            lock.notifyAll();
            len -= lenToWrite;
            off += lenToWrite;
            capacityRemaining -= lenToWrite;
            }
        }
    }



    public int read( byte[] arr, int off, int len ) {
        len = Math.min( len, arr.length-off );
        if (len<=0) return 0;
        synchronized (lock) {
            while ( isEmpty() ) {
                if (isClosing) {
                    //L0g.log("read()<0");
                    return -1;
                }
                try { lock.wait(500); }
                catch ( InterruptedException interrupt ) { break; }
            }
            int readCount = cbb.removeFirst(arr,off,len);
            if (readCount>0) lock.notifyAll();
            //L0g.log( "read(arr,"+off+","+len+"):"+new String(arr,off,readCount) );
            return readCount;
        }
    }



    public String toString() {
        synchronized (lock) {
            return cbb.toString();
        }
    }



    public static boolean _TEST_() throws Exception {
        String s = """
            Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed non risus. Suspendisse lectus tortor,
            dignissim sit amet, adipiscing nec, ultricies sed, dolor. Cras elementum ultrices diam. Maecenas ligula
            massa, varius a, semper congue, euismod non, mi. Proin porttitor, orci nec nonummy molestie, enim est
            eleifend mi, non fermentum diam nisl sit amet erat. Duis semper. Duis arcu massa, scelerisque vitae,
            consequat in, pretium a, enim. Pellentesque congue. Ut in risus volutpat libero pharetra tempor.
            Cras vestibulum bibendum augue. Praesent egestas leo in pede. Praesent blandit odio eu enim.
            Pellentesque sed dui ut augue blandit sodales. Vestibulum ante ipsum primis in faucibus orci luctus et
            ultrices posuere cubilia Curae; Aliquam nibh. Mauris ac mauris sed pede pellentesque fermentum.
            Maecenas adipiscing ante non diam sodales hendrerit. Ut velit mauris, egestas sed, gravida nec, ornare
            ut, mi. Aenean ut orci vel massa suscipit pulvinar. Nulla sollicitudin. Fusce varius, ligula non tempus
            aliquam, nunc turpis ullamcorper nibh, in tempus sapien eros vitae ligula. Pellentesque rhoncus nunc et
            augue. Integer id felis. Curabitur aliquet pellentesque diam. Integer quis metus vitae elit lobortis
            egestas. Lorem ipsum dolor sit amet, consectetuer adipiscing elit. Morbi vel erat non mauris convallis
            vehicula. Nulla et sapien. Integer tortor tellus, aliquam faucibus, convallis id, congue eu, quam.
            Mauris ullamcorper felis vitae erat. Proin feugiat, augue non elementum posuere, metus purus iaculis
            lectus, et tristique ligula justo vitae magna. Aliquam convallis sollicitudin purus.
        """;
        final boolean[] fail = new boolean[]{false};
        BlockingByteBuffer bbb = new BlockingByteBuffer( s.length() / 10 );
        Thread writerThread = new Thread(){
            public void run() {
                OutputStream os = bbb.getOutputStream();
                int wroteCount = 0;
                while ( wroteCount < s.length() ) {
                    int writeCount = Math.min( 1 + (int)(Math.random()*10), s.length()-wroteCount );
                    byte[] arr = s.substring( wroteCount, wroteCount+writeCount ).getBytes();
                    try{ os.write( arr, 0, arr.length ); }
                    catch ( IOException e ) { fail[0]=true; throw new RuntimeException(e); }
                    wroteCount += writeCount;
                }
                try{ os.close(); }catch (IOException e){ fail[0]=true; throw new RuntimeException(e); }
            }
        };
        writerThread.start();
        Thread.sleep(100);
        StringBuffer sb = new StringBuffer();
        byte[] arr = new byte[100];
        InputStream is = bbb.getInputStream();
        while ( sb.length() < s.length() ) {
            int readCount = is.read( arr, 0, arr.length );
            if (readCount<0) throw new RuntimeException("Unexpected end of stream. (1)");
            sb.append( new String(arr,0,readCount) );
        }
        writerThread.join();
        LibTest.asrtEQ( sb.toString(), s );
        bbb.getOutputStream().close();
        LibTest.asrtEQ( -1, bbb.getInputStream().read() );
        return ! fail[0];
    }



    public static void main( String[] args ) throws Exception { LibTest.testClass(); }
}
