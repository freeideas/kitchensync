package jLib;
import java.util.*;
import java.lang.reflect.Field;



public interface JsonSerializable {



    default public CharSequence toJson() {
        return JsonEncoder.encode(this);
    }



    /**
     * NOTE: This default implementation requires a no-arg public constructor,
     * and it uses reflection to set the fields. Feel free to override.
     */
    public static Object fromJson( CharSequence json ) {
        Map<?,?> map = JsonDecoder.decodeMap(json);
        String className = (String) map.get( "__class__" );
        if (className==null) return null;
        try {
            Class<?> clazz = Class.forName(className);
            Object obj = clazz.getDeclaredConstructor().newInstance();
            for ( Map.Entry<?,?> entry : map.entrySet() ) {
                String fieldName = (String) entry.getKey();
                if (fieldName.equals("__class__")) continue;
                Object fieldValue = entry.getValue();
                Field field = clazz.getDeclaredField(fieldName);
                boolean accessible = field.canAccess(obj);
                if (!accessible) field.setAccessible(true);
                field.set(obj, fieldValue);
                if (!accessible) field.setAccessible(false);
            }
            return obj;
        }
        catch (Exception e) {
            if (e instanceof RuntimeException re) throw re;
            throw new RuntimeException(e);
        }
    }



    @SuppressWarnings("unused")
    private static boolean jsonSerializable_TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        TestClass obj = new TestClass("Alice", 25);
        CharSequence json = obj.toJson();
        TestClass obj2 = (TestClass) JsonSerializable.fromJson(json);
        LibTest.asrtEQ( obj.name, obj2.name );
        LibTest.asrtEQ( obj.age, obj2.age );
        return true;
    }



    public static void main( String[] args ) { LibTest.testClass(); }
}



class TestClass implements JsonSerializable {
    public static String ok="ok";
    public String name;
    public int age;
    public TestClass() {}
    public TestClass( String name, int age ) {
        this.name = name;
        this.age = age;
    }
}
