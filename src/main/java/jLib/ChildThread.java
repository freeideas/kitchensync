package jLib;


/*
 * a thread that remembers its parent thread, and can be closed
 */
@SuppressWarnings("this-escape")
public class ChildThread extends Thread implements AutoCloseable {
    { Lib.cleaner.register( this, ()->close() ); }
    private Thread parent;
    private String internalName = this.getClass().getSimpleName()+"_"+Lib.timeStamp();
    public ChildThread() { parent = Thread.currentThread(); }
    public ChildThread( Runnable r ) {
        super(r);
        parent = Thread.currentThread();
        setName(internalName);
        setDaemon(true);
    }
    @Override
    public void start() {
        parent = Thread.currentThread();
        super.start();
    }
    public Thread getParent() { return parent; }
    @Override public void close() { interrupt(); }
    public String toString() { return getName(); }
    public int hashCode() { return internalName.hashCode(); }
    public boolean equals( Object o ) { return o==this; }
}