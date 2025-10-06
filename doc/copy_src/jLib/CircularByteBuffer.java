package jLib;

import java.io.*;
import java.nio.charset.*;

public class CircularByteBuffer implements Cloneable {
    private byte[] buffer;
    private int head;
    private int size;
    private byte[] miniBuf = new byte[1];

    public CircularByteBuffer() {
        this(512);
    }

    public CircularByteBuffer(int capacity) {
        buffer = new byte[capacity];
        head = 0;
        size = 0;
    }

    public byte[] getBytes() {
        byte[] result = new byte[size];
        get(0,result,0,size);
        return result;
    }

    public CircularByteBuffer clone() {
        CircularByteBuffer c = new CircularByteBuffer(buffer.length);
        c.head = head;
        c.size = size;
        System.arraycopy(buffer, 0, c.buffer, 0, buffer.length);
        return c;
    }

    public CircularByteBuffer identicalClone() {
        CircularByteBuffer c = new CircularByteBuffer(buffer.length);
        c.head = head;
        c.size = size;
        System.arraycopy(buffer, 0, c.buffer, 0, buffer.length);
        return c;
    }

    public int size() {
        return size;
    }

    public boolean isEmpty() {
        return size == 0;
    }

    public int capacity() {
        return buffer.length;
    }

    public int spareCapacity() {
        return buffer.length - size;
    }

    public byte get(int idx) {
        if (idx < 0)
            idx = size + (idx % size);
        if (idx >= size)
            idx %= size;
        idx += head;
        if (idx >= buffer.length)
            idx -= buffer.length;
        return buffer[idx];
    }

    public int get(int idx, byte[] arr, int off, int len) {
        len = Math.min(len, size - idx);
        if (len==0) return 0;
        int result = len;
        // read contiguous block from head+idx to end of buffer
        int bufIdx = (head + idx) % buffer.length;
        int blockLen = Math.min(len, buffer.length - bufIdx);
        System.arraycopy(buffer, bufIdx, arr, off, blockLen);
        off += blockLen;
        len -= blockLen;
        if (len==0) return result;
        // read contiguous block from start of buffer
        System.arraycopy(buffer, 0, arr, off, len);
        return result;
    }

    public boolean regionEquals( int idx, byte[] arr ) {
        return regionEquals( idx, arr, 0, arr.length );
    }
    public boolean regionEquals( int idx, byte[] arr, int off, int len ) {
        len = Math.min( len, arr.length-off );
        if ( len+idx > size ) return false;
        for ( int i=0; i<len; i++ ) {
            if ( arr[i+off] != get(i+idx) ) return false;
        }
        return true;
    }

    public boolean startsWith( byte[] arr ) {
        return regionEquals( 0, arr );
    }
    public boolean startsWith( byte[] arr, int off, int len ) {
        return regionEquals( 0, arr, off, len );
    }

    public boolean endsWith( byte[] arr ) {
        return regionEquals( size-arr.length, arr );
    }
    public boolean endsWith( byte[] arr, int off, int len ) {
        return regionEquals( size-len, arr, off, len );
    }

    public int indexOf( byte[] arr ) {
        return indexOf( arr, 0, arr.length );
    }
    public int indexOf( byte[] arr, int off, int len ) {
        if ( len > size ) return -1;
        for ( int i=0; i<=size-len; i++ ) if ( regionEquals(i,arr,off,len) ) return i;
        return -1;
    }

    public byte set(int idx, byte x) {
        if (idx < 0)
            idx = size + (idx % size);
        if (idx >= size)
            idx %= size;
        idx += head;
        if (idx >= buffer.length)
            idx -= buffer.length;
        byte old = buffer[idx];
        buffer[idx] = x;
        return old;
    }

    public int set(int idx, byte[] arr, int off, int len) {
        if (idx + len > size)
            len = size - idx;
        if (len == 0)
            return 0;
        int result = len;
        // write into contiguous block from head+idx to end of buffer
        int blockLen = Math.min(len, buffer.length - (head + idx) % buffer.length);
        System.arraycopy(arr, off, buffer, (head + idx) % buffer.length, blockLen);
        off += blockLen;
        len -= blockLen;
        if (len == 0)
            return result;
        // write into contiguous block from start of buffer
        System.arraycopy(arr, off, buffer, 0, len);
        return result;
    }

    public int removeFirst(byte[] arr, int off, int len) {
        int result = get(0, arr, off, len);
        if (result<=0) return result;
        head = (head + result) % buffer.length;
        size -= result;
        return result;
    }

    public int removeFirst(byte[] arr) {
        return removeFirst(arr, 0, arr.length);
    }

    public byte removeFirst() {
        if (size == 0)
            throw new IllegalStateException("Buffer is empty");
        byte b = buffer[head];
        head++;
        if (head >= buffer.length)
            head = 0;
        size--;
        return b;
    }

    public void addLast(byte[] arr, int off, int len) {
        ensureCapacity( len+size );
        size += len;
        set(size - len, arr, off, len);
    }

    public void addLast(byte[] arr) {
        addLast(arr, 0, arr.length);
    }

    public void addLast(byte x) {
        miniBuf[0] = x;
        addLast(miniBuf);
    }

    public void addFirst(byte[] arr, int off, int len) {
        ensureCapacity( len+size );
        size += len;
        head = head - len;
        if (head < 0)
            head = buffer.length + head;
        set(0, arr, off, len);
    }

    public void addFirst(byte[] arr) {
        addFirst(arr, 0, arr.length);
    }

    public void addFirst(byte x) {
        miniBuf[0] = x;
        addFirst(miniBuf);
    }

    public int removeLast(byte[] arr, int off, int len) {
        if (size == 0)
            return 0;
        len = Math.min(len, size);
        int result = get(size - len, arr, off, len);
        size -= result;
        return result;
    }

    public int removeLast(byte[] arr) {
        return removeLast(arr, 0, arr.length);
    }

    public byte removeLast() {
        if (size == 0) {
            throw new IllegalStateException("Buffer is empty");
        }
        byte x = buffer[(head + size - 1) % buffer.length];
        size--;
        return x;
    }

    public int ensureCapacity( int minSize ) {
        if ( buffer.length >= minSize ) return buffer.length;
        if ( buffer.length < 128 ) return setCapacity( Math.max(minSize,128) );
        if ( buffer.length < 1024*1024 ) return setCapacity(  Math.max(minSize,buffer.length*2) );
        int tryCapacity = Math.max( minSize, buffer.length + buffer.length/2 );
        if (tryCapacity<=0) tryCapacity = Integer.MAX_VALUE;
        return setCapacity( tryCapacity );
    }

    public int setCapacity(int newCapacity) {
        if (newCapacity < size) newCapacity = size;
        byte[] newBuffer = new byte[newCapacity];
        get(0,newBuffer,0,size);
        buffer = newBuffer;
        head = 0;
        return newCapacity;
    }

    public CircularByteBuffer clear() {
        head = 0;
        size = 0;
        return this;
    }

    public int skipFirst( int n ) {
        if (n>size) n = size;
        head = (head+n) % buffer.length;
        size -= n;
        return n;
    }
    public int skipLast( int n ) {
        if (n>size) n = size;
        size -= n;
        return n;
    }

    public int readFrom( InputStream inp, int maxReadCount ) throws IOException {
        maxReadCount = maxReadCount > 0 ? maxReadCount : 1024;
        if ( size==buffer.length ) ensureCapacity( buffer.length + 1024 );
        int freeRegionTopIndex = (head+size) % buffer.length;
        int freeRegionEndIndex = freeRegionTopIndex + Math.min( maxReadCount, spareCapacity() );
        if ( freeRegionEndIndex > buffer.length ) freeRegionEndIndex = buffer.length;
        int freeRegionLen = freeRegionEndIndex - freeRegionTopIndex;
        int readCount = inp.read( buffer, freeRegionTopIndex, freeRegionLen );
        if (readCount>0) size += readCount;
        return readCount;
    }

    public void writeTo( OutputStream out, int maxWriteCount ) throws IOException {
        if (maxWriteCount<=0) maxWriteCount = size;
        int writeCount = Math.min( maxWriteCount, size );
        byte[] toWrite = new byte[writeCount];
        removeFirst(toWrite);
        out.write(toWrite);
    }

    public OutputStream getOutputStream() {
        return new OutputStream() {
            @Override public void write(int b) {
                addLast((byte) b);
            }
            @Override public void write(byte[] b, int off, int len) {
                addLast(b, off, len);
            }
            @Override public void write(byte[] b) {
                addLast(b);
            }
        };
    }

    public InputStream getInputStream() {
        // consumes data
        return new InputStream() {
            @Override public int read() {
                if (size<=0) return -1;
                return removeFirst();
            }
            @Override public int read(byte[] b, int off, int len) {
                if (size<=0) return -1;
                int result = removeFirst(b,off,len);
                return result;
            }
            @Override public int read(byte[] b) {
                if (size<=0) return -1;
                int result = removeFirst(b);
                return result;
            }
            @Override public int available() {
                return size;
            }
            @Override
            public long skip( long n ) {
                int toSkip = Math.min( (int)n, size );
                return skipFirst(toSkip);
            }
        };
    }

    public CharSequence asCharSequence() {
        return asCharSequence(0,-1);
    }
    public CharSequence asCharSequence( int start, int end ) {
        return new CharSequence() {
            @Override
            public int length() {
                int len = size-start;
                if (end>=0) len = Math.min( len, end-start );
                return len;
            }
            @Override
            public char charAt( int index ) {
                if ( index >= length() ) throw new IndexOutOfBoundsException();
                int cbbIndex = start + index ;
                return (char) get(cbbIndex);
            }
            @Override
            public CharSequence subSequence( int st, int en ) {
                if (en>=0) en+=start;
                return asCharSequence( start+st, en );
            }
            @Override
            public String toString() {
                StringBuilder sb = new StringBuilder();
                int len = length();
                for (int i = 0; i<len; i++) sb.append( charAt(i) );
                return sb.toString();
            }
        };
    }

    public String toString( Charset charset ) {
        if (charset==null) charset = StandardCharsets.UTF_8;
        return new String( getBytes(), charset );
    }

    @Override
    public String toString() {
        return toString(null);
    }

    @Override
    public int hashCode() {
        int hash = 0;
        for (int i = 0; i < size; i++) {
            Object obj = get(i);
            if (obj != null)
                hash += obj.hashCode();
        }
        return hash;
    }

    @Override
    public boolean equals(Object o) {
        if (o == null)
            return false;
        if (o == this)
            return true;
        if (o.getClass() != this.getClass())
            return false;
        CircularByteBuffer other = (CircularByteBuffer) o;
        if (other.size != this.size)
            return false;
        for (int i = 0; i < size; i++) {
            byte bA = get(i);
            byte bB = other.get(i);
            if (bA != bB)
                return false;
        }
        return true;
    }

    private static boolean _TEST_() throws Exception {
        {
            CircularByteBuffer cbb = new CircularByteBuffer();
            cbb.addLast( " world!".getBytes() );
            cbb.addFirst( "Hello".getBytes() );
            LibTest.asrt( cbb.regionEquals( 3, "Flo wok".getBytes(), 1, 5 ) );
            CharSequence cs = cbb.asCharSequence();
            java.util.regex.Pattern pat = java.util.regex.Pattern.compile( "Hello\\s+world!" );
            java.util.regex.Matcher mat = pat.matcher(cs);
            LibTest.asrt( mat.matches() );
            cbb.skipFirst(1);
            cbb.skipLast(1);
            String expect = "ello world";
            LibTest.asrt( cbb.asCharSequence(), expect );
            cs = cbb.asCharSequence( 1, expect.length()-1 );
            expect = expect.subSequence( 1, expect.length()-1 ).toString();
            LibTest.asrtEQ(cs,expect);
            cs = cs.subSequence( 1, cs.length()-1 );
            expect = expect.subSequence( 1, expect.length()-1 ).toString();
            LibTest.asrt(cs,expect);
        }
        { // test readFrom()
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
            ByteArrayInputStream inp = new ByteArrayInputStream( s.getBytes() );
            CircularByteBuffer cbb = new CircularByteBuffer(8);
            while ( cbb.size() < s.length() ) cbb.readFrom( inp, (int)(Math.random()*10) );
            LibTest.asrt( cbb.asCharSequence(), s );
        }
        return true;
    }

    public static void main( String[] args ) throws Exception { LibTest.testClass(); }
}
