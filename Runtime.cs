using System;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Runtime.CompilerServices;
using System.Runtime.Loader;
using System.Text;

class Utils
{
    public static string ReadUtf8Z(IntPtr p)
    {
        if (p == IntPtr.Zero) return string.Empty;
        return Marshal.PtrToStringUTF8(p)!;
    }

    static IntPtr AllocUtf8(string s, out int len)
    {
        // allocate CoTaskMem UTF-8 (host must call FreeMemory)
        var bytes = Encoding.UTF8.GetBytes(s);
        len = bytes.Length;
        var dst = Marshal.AllocCoTaskMem(len + 1);
        Marshal.Copy(bytes, 0, dst, len);
        Marshal.WriteByte(dst, len, 0);
        return dst;
    }

    public static IntPtr Pin(object obj, bool pinned = false) => GCHandle.ToIntPtr(GCHandle.Alloc(obj, pinned ? GCHandleType.Pinned : GCHandleType.Normal));

    public static void Unpin(IntPtr id) => GCHandle.FromIntPtr(id).Free();

    public static T? Ref<T>(IntPtr target) => (T?)GCHandle.FromIntPtr(target).Target;
}

public class RuntimeAssembly
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void GetClassDelegate(Assembly assembly, IntPtr typeNameUtf8Z, out IntPtr result); // returns type handle
    public static void GetClass(Assembly assembly, IntPtr typeNameUtf8Z, out IntPtr result)
    {
        var tn = Utils.ReadUtf8Z(typeNameUtf8Z);
        var t = ResolveTypeInAsm(assembly, tn) ?? throw new TypeLoadException($"Type not found: {tn}");
        if (t == null) {
            result = IntPtr.Zero;
        } else {
            result = Utils.Pin(t);
        }
    }

    static Type? ResolveTypeInAsm(Assembly assembly, string fullOrShort)
    {
        var t = Type.GetType(fullOrShort, throwOnError: false, ignoreCase: false);
        if (t != null) return t;

        t = assembly.GetType(fullOrShort, throwOnError: false, ignoreCase: false);
        if (t != null) return t;

        return assembly.GetTypes().FirstOrDefault(x => string.Equals(x.FullName, fullOrShort, StringComparison.Ordinal));
    }

    public static Type? ResolveTypeInContext(Scope scope, string fullOrShort) => null;
}

public class RuntimeClass
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void NewDelegate(IntPtr klass, out IntPtr result);
    public static void New(IntPtr klass, out IntPtr result)
    {
        var t = Utils.Ref<Type>(klass) ?? throw new ArgumentNullException("Class is null");
        var obj = Activator.CreateInstance(t) ?? throw new MissingMethodException($"No default ctor for {t.FullName}");
        result = Utils.Pin(obj);
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void IsAssignableFromDelegate(IntPtr baseKlass, IntPtr targetKlass, out int result);
    public static void IsAssignableFrom(IntPtr baseKlass, IntPtr targetKlass, out int result)
    {
        var baseType = Utils.Ref<Type>(baseKlass) ?? throw new ArgumentNullException("Base class is null");
        var targetType = Utils.Ref<Type>(targetKlass) ?? throw new ArgumentNullException("Target class is null");
        result = baseType.IsAssignableFrom(targetType) ? 1 : 0;
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void GetMethodDelegate(IntPtr klass, IntPtr nameUtf8Z, int argCount, out IntPtr result);
    public static void GetMethod(IntPtr klass, IntPtr name, int argCount, out IntPtr result)
    {
        var t = Utils.Ref<Type>(klass) ?? throw new ArgumentNullException("Class is null");
        var methodName = Utils.ReadUtf8Z(name);

        var flags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.Instance;
        var cand = t.GetMethods(flags).Where(m => m.Name == methodName && m.GetParameters().Length == argCount).FirstOrDefault<MethodInfo>();

        result = cand == null ? IntPtr.Zero : Utils.Pin(cand);
    }
}

public class RuntimeMethod
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public unsafe delegate void RuntimeInvokeDelegate(IntPtr method, void* instance, void** args);
    public unsafe static void RuntimeInvoke(IntPtr method, void* instancePtr, void** argv)
    {
        Console.WriteLine("Runtime invoking method");

        var m = Utils.Ref<MethodInfo>(method) ?? throw new ArgumentNullException("Method is null");

        Console.WriteLine("  - Get params and arg length");
        var parameters = m.GetParameters();
        var argc = parameters.Length;

        object? instance = null;
        if (instancePtr != null)
        {
            Console.WriteLine("  - Unbox instance");
            instance = Utils.Ref<object>((IntPtr)instancePtr);
        }

        Console.WriteLine($"  - Unbox args [{argc}]");
        var args = new object?[argc];
        for (var i = 0; i < argc; i++)
        {
            Console.WriteLine($"    - {parameters[i].ParameterType.FullName}");
            if (parameters[i].ParameterType.IsValueType) {
                Console.WriteLine($"    - value null:{argv[i]==null}");
                args[i] = ReadValueAsObject(argv[i], parameters[i].ParameterType);
                Console.WriteLine("    - complete");
            } else if (parameters[i].ParameterType == typeof(string)) {
                Console.WriteLine("    - string");
                args[i] = Marshal.PtrToStringUTF8((IntPtr)argv[i]);
                Console.WriteLine("    - complete");
            } else {
                Console.WriteLine("    - GCHandle");
                args[i] = GCHandle.FromIntPtr((IntPtr)argv[i]).Target;
                Console.WriteLine("    - complete");
            }
        }

        Console.WriteLine("  - Invoke");
        m.Invoke(instance, args);
    }

    private static unsafe object? ReadValueAsObject(void* p, Type t)
    {
        var elem = t.IsByRef ? t.GetElementType() : t;

        if (elem == typeof(IntPtr) || elem == typeof(nint))
        {
            nint val = p == null ? default : Unsafe.Read<nint>(p);
            return (IntPtr)val;
        }

        // Fast path for common primitives
        if (elem == typeof(int)) return Unsafe.Read<int>(p);
        if (elem == typeof(uint))   return Unsafe.Read<uint>(p);
        if (elem == typeof(long))   return Unsafe.Read<long>(p);
        if (elem == typeof(ulong))  return Unsafe.Read<ulong>(p);
        if (elem == typeof(short))  return Unsafe.Read<short>(p);
        if (elem == typeof(ushort)) return Unsafe.Read<ushort>(p);
        if (elem == typeof(byte))   return Unsafe.Read<byte>(p);
        if (elem == typeof(sbyte))  return Unsafe.Read<sbyte>(p);
        if (elem == typeof(bool))   return Unsafe.Read<byte>(p) != 0;       // define size!
        if (elem == typeof(float))  return Unsafe.Read<float>(p);
        if (elem == typeof(double)) return Unsafe.Read<double>(p);
        if (elem != null && elem.IsEnum)
        {
            var u = Enum.GetUnderlyingType(t) ?? typeof(int);
            object? raw = ReadValueAsObject(p, u);
            raw = Convert.ChangeType(raw, u);
            return raw == null ? null : Enum.ToObject(t, raw);
        }

        // Blittable structs → Marshal.PtrToStructure (boxed)
        // (You can replace with Unsafe.Read<T> if you constrain to blittable T known at compile time.)
        return Marshal.PtrToStructure((IntPtr)p, t)!;
    }
}

public sealed class Scope : AssemblyLoadContext
{
    public readonly string BaseDir;

    public Scope(string baseDir) : base(isCollectible: true)
    {
        BaseDir = baseDir;
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void LoadFromPathDelegate(IntPtr scopeId, IntPtr pathUtf8Z, out Assembly result);
    public static void LoadFromPath(IntPtr scope, IntPtr path, out Assembly? result)
    {
        var self = Utils.Ref<Scope>(scope) ?? throw new ArgumentNullException("Scope is null");
        var p = Path.Combine(self.BaseDir, Utils.ReadUtf8Z(path));
        try
        {
            result = self.LoadFromAssemblyPath(p);
        }
        catch
        {
            result = null;
        }
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void LoadFromBytesDelegate(IntPtr scopeId, IntPtr bytes, int length, out Assembly result);
    public static void LoadFromBytes(IntPtr scope, IntPtr bytes, int length, out Assembly? result)
    {
        unsafe
        {
            var self = Utils.Ref<Scope>(scope) ?? throw new ArgumentNullException("Scope is null");
            var span = new ReadOnlySpan<byte>((void*)bytes, length);
            using var ms = new MemoryStream(span.ToArray());
            try
            {
                result = self.LoadFromStream(ms);
            }
            catch
            {
                result = null;
            }
        }
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void UnloadDelegate(IntPtr scopeId);
    public static void Unload(IntPtr scope)
    {
        var handle = GCHandle.FromIntPtr(scope);
        var target = (Scope?)handle.Target ?? throw new ArgumentNullException("Scope is null");

        target.Unload();
        handle.Free();

        GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
    }
}

public class Host
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void PingDelegate(out uint result);
    public static void Ping(out uint result) => result = 1;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void CreateScopeDelegate(IntPtr baseDir, out IntPtr result);
    public static void CreateScope(IntPtr baseDir, out IntPtr result)
    {
        // Use current assembly directory for probing by default
        var dir = baseDir == IntPtr.Zero ? Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location)! : Utils.ReadUtf8Z(baseDir);
        result = Utils.Pin(new Scope(dir));
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void DestroyDelegate(IntPtr handle);  // frees any object/type/assembly handle
    public static void Destroy(IntPtr handle) => Utils.Unpin(handle);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void FreeDelegate(IntPtr ptr);
    public static void Free(IntPtr ptr)
    {
        if (ptr != IntPtr.Zero) Marshal.FreeCoTaskMem(ptr);
    }
}
