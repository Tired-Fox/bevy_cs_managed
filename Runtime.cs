using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Runtime.CompilerServices;
using System.Runtime.Loader;
using System.Text;

class Field {
    public string Name { get; set; } = default;
    public bool IsStatic { get; set; }
    public object[] CustomAttributes { get; set; } = default;
}

class Property {
    public string Name { get; set; } = default;
    public bool IsStatic { get; set; }
    public object[] CustomAttributes { get; set; } = default;
    public bool CanRead { get; set; }
    public bool CanWrite { get; set; }
}

public enum Error {
    ClassNotFound = 1,
    MethodNotFound = 2,
    FieldNotFound = 3,
    PropertyNotFound = 4,
    ReadonlyField = 5,
    MissingGetter = 6,
    MissingSetter = 7,
    MissingRequiredArgument = 8,
    PathNotFound = 9,
    AssemblyNotLoaded = 10,
    ClassNotRegistered = 11,
}

public sealed class Scope : AssemblyLoadContext
{
    public readonly string BaseDir;

    public Scope(string baseDir) : base(isCollectible: true)
    {
        BaseDir = baseDir;
    }
}

public class Host
{
    static string ReadUtf8Z(IntPtr p)
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

    static IntPtr Pin(object obj, bool pinned = false) => GCHandle.ToIntPtr(GCHandle.Alloc(obj, pinned ? GCHandleType.Pinned : GCHandleType.Normal));
    static void Unpin(IntPtr id) => GCHandle.FromIntPtr(id).Free();
    static T? Ref<T>(IntPtr target) => (T?)GCHandle.FromIntPtr(target).Target;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void PingDelegate(out uint result);
    public static void Ping(out uint result) => result = 1;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void DestroyDelegate(IntPtr handle);  // frees any object/type/assembly handle
    public static void Destroy(IntPtr handle) => Unpin(handle);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void FreeDelegate(IntPtr ptr);
    public static void Free(IntPtr ptr)
    {
        if (ptr != IntPtr.Zero) Marshal.FreeHGlobal(ptr);
    }

    // ----- SCOPE -----

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void CreateScopeDelegate(IntPtr baseDir, out IntPtr result);
    public static void CreateScope(IntPtr baseDir, out IntPtr result)
    {
        // Use current assembly directory for probing by default
        var dir = baseDir == IntPtr.Zero ? Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location)! : ReadUtf8Z(baseDir);
        result = Pin(new Scope(dir));
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void LoadFromPathDelegate(IntPtr scopeId, IntPtr pathUtf8Z, out Assembly? result, out int error);
    public static void LoadFromPath(IntPtr scope, IntPtr path, out Assembly? result, out int error)
    {
        result = null;
        error = 0;

        var self = Ref<Scope>(scope);
        if (self == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }

        var p = Path.Combine(self.BaseDir, ReadUtf8Z(path));
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
    public delegate void LoadFromBytesDelegate(IntPtr scopeId, IntPtr bytes, int length, out Assembly result, out int error);
    public static void LoadFromBytes(IntPtr scope, IntPtr bytes, int length, out Assembly? result, out int error)
    {
        result = null;
        error = 0;

        unsafe
        {
            var self = Ref<Scope>(scope);
            if (self == null) {
                error = (int)Error.MissingRequiredArgument;
                return;
            }

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
    public delegate void UnloadDelegate(IntPtr scopeId, out int error);
    public static void Unload(IntPtr scope, out int error)
    {
        error = 0;

        var handle = GCHandle.FromIntPtr(scope);
        var target = (Scope?)handle.Target;
        if (target == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }

        target.Unload();
        handle.Free();

        GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
    }

    // ----- CLASS -----

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void GetClassDelegate(Assembly assembly, IntPtr typeNameUtf8Z, out IntPtr result, out int error); // returns type handle
    public static void GetClass(Assembly assembly, IntPtr typeNameUtf8Z, out IntPtr result, out int error)
    {
        result = IntPtr.Zero;
        error = 0;

        var tn = ReadUtf8Z(typeNameUtf8Z);
        var t = ResolveTypeInAsm(assembly, tn);
        if (t == null) {
            error = (int)Error.ClassNotFound;
            return;
        }

        if (t == null) {
            result = IntPtr.Zero;
        } else {
            result = Pin(t);
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

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void NewDelegate(IntPtr klass, out IntPtr result, out int error);
    public static void New(IntPtr klass, out IntPtr result, out int error)
    {
        result = IntPtr.Zero;
        error = 0;

        var t = Ref<Type>(klass);
        if (t == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }

        var obj = Activator.CreateInstance(t);
        if (obj == null) {
            error = (int)Error.MethodNotFound;
            return;
        }

        result = Pin(obj);
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void IsAssignableFromDelegate(IntPtr baseKlass, IntPtr targetKlass, out int result, out int error);
    public static void IsAssignableFrom(IntPtr baseKlass, IntPtr targetKlass, out int result, out int error)
    {
        result = 0;
        error = 0;

        var baseType = Ref<Type>(baseKlass);
        if (baseType == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }
        var targetType = Ref<Type>(targetKlass);
        if (targetType == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }
        result = baseType.IsAssignableFrom(targetType) ? 1 : 0;
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void GetMetaDataDelegate(IntPtr klass, out IntPtr result, out int error);
    public static void GetMetaData(IntPtr klass, out IntPtr result, out int error)
    {
        result = IntPtr.Zero;
        error = 0;

        var t = Ref<Type>(klass);
        if (t == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }

        var fields = t.GetFields()
            .Select(f => new Field {
                Name = f.Name,
                IsStatic = f.IsStatic,
                CustomAttributes = f.GetCustomAttributes(false).ToArray()
            })
            .ToList();

        var properties = t.GetProperties()
            .Select(p => {
                bool isStatic = (p.GetGetMethod(true)?.IsStatic ?? false) ||
                    (p.GetSetMethod(true)?.IsStatic ?? false);

                return new Property {
                    Name = p.Name,
                    IsStatic = isStatic,
                    CustomAttributes = p.GetCustomAttributes(false).ToArray(),
                    CanRead = p.CanRead,
                    CanWrite = p.CanWrite,
                };
            })
            .ToList();

        string response = JsonSerializer.Serialize(new {
            Fields = fields,
            Properties = properties,
        });

        byte[] bytes = System.Text.Encoding.UTF8.GetBytes(response);

        result = Marshal.AllocHGlobal(bytes.Length + 1);
        Marshal.Copy(bytes, 0, result, bytes.Length);
        Marshal.WriteByte(result, bytes.Length, 0);
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void GetMethodDelegate(IntPtr klass, IntPtr nameUtf8Z, int argCount, out IntPtr result, out int error);
    public static void GetMethod(IntPtr klass, IntPtr name, int argCount, out IntPtr result, out int error)
    {
        result = IntPtr.Zero;
        error = 0;

        var t = Ref<Type>(klass);
        if (t == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }
        var methodName = ReadUtf8Z(name);

        var flags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.Instance;
        var cand = t.GetMethods(flags).Where(m => m.Name == methodName && m.GetParameters().Length == argCount).FirstOrDefault<MethodInfo>();

        result = cand == null ? IntPtr.Zero : Pin(cand);
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public unsafe delegate void SetFieldValueDelegate(IntPtr instance, IntPtr name, void* value, out int error);
    public unsafe static void SetFieldValue(IntPtr instance, IntPtr name, void* value, out int error)
    {
        error = 0;

        var target = Ref<object>(instance);
        if (target == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }
        var fieldName = ReadUtf8Z(name);
        var flags = BindingFlags.Instance | BindingFlags.Public;

        var fi = target.GetType().GetField(fieldName, flags);
        if (fi == null) {
            error = (int)Error.FieldNotFound;
            return;
        }

        if ((fi.Attributes & FieldAttributes.InitOnly) != 0) {
            error = (int)Error.ReadonlyField;
            return;
        }

        var fv = ReadValueAsObject(value, fi.FieldType);
        if (fi.IsStatic) {
            fi.SetValue(null, fv);
        } else {
            fi.SetValue(target, fv);
        }
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public unsafe delegate void GetFieldValueDelegate(IntPtr instance, IntPtr name, out IntPtr result, out int error);
    public unsafe static void GetFieldValue(IntPtr instance, IntPtr name, out IntPtr result, out int error)
    {
        result = IntPtr.Zero;
        error = 0;

        var target = Ref<object>(instance);
        if (target == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }
        var fieldName = ReadUtf8Z(name);
        var flags = BindingFlags.Instance | BindingFlags.Public;

        var fi = target.GetType().GetField(fieldName, flags);
        if (fi == null) {
            error = (int)Error.FieldNotFound;
            return;
        }
        var value = fi.GetValue(target);

        var options = new JsonSerializerOptions { IncludeFields = true };
        string response = JsonSerializer.Serialize(value, options);

        byte[] bytes = System.Text.Encoding.UTF8.GetBytes(response);

        result = Marshal.AllocHGlobal(bytes.Length + 1);
        Marshal.Copy(bytes, 0, result, bytes.Length);
        Marshal.WriteByte(result, bytes.Length, 0);
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public unsafe delegate void SetPropertyValueDelegate(IntPtr instance, IntPtr name, void* value, out int error);
    public unsafe static void SetPropertyValue(IntPtr instance, IntPtr name, void* value, out int error)
    {
        error = 0;

        var target = Ref<object>(instance);
        if (target == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }
        var propertyName = ReadUtf8Z(name);
        var flags = BindingFlags.Instance | BindingFlags.Public;

        var fi = target.GetType().GetProperty(propertyName, flags);
        if (fi == null) {
            error = (int)Error.PropertyNotFound;
            return;
        }

        if (!fi.CanWrite) {
            error = (int)Error.MissingSetter;
            return;
        }

        bool isStatic = (fi.GetGetMethod(true)?.IsStatic ?? false) ||
            (fi.GetSetMethod(true)?.IsStatic ?? false);

        var fv = ReadValueAsObject(value, fi.PropertyType);
        if (isStatic) {
            fi.SetValue(null, fv);
        } else {
            fi.SetValue(target, fv);
        }
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public unsafe delegate void GetPropertyValueDelegate(IntPtr instance, IntPtr name, out IntPtr result, out int error);
    public unsafe static void GetPropertyValue(IntPtr instance, IntPtr name, out IntPtr result, out int error)
    {
        result = IntPtr.Zero;
        error = 0;

        var target = Ref<object>(instance);
        if (target == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }
        var fieldName = ReadUtf8Z(name);
        var flags = BindingFlags.Instance | BindingFlags.Public;

        var fi = target.GetType().GetProperty(fieldName, flags);
        if (fi == null) {
            error = (int)Error.PropertyNotFound;
            return;
        }

        if (!fi.CanRead) {
            error = (int)Error.MissingGetter;
            return;
        }

        var value = fi.GetValue(target);

        var options = new JsonSerializerOptions { IncludeFields = true };
        string response = JsonSerializer.Serialize(value, options);

        byte[] bytes = System.Text.Encoding.UTF8.GetBytes(response);

        result = Marshal.AllocHGlobal(bytes.Length + 1);
        Marshal.Copy(bytes, 0, result, bytes.Length);
        Marshal.WriteByte(result, bytes.Length, 0);
    }

    // ----- METHOD -----

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public unsafe delegate void RuntimeInvokeDelegate(IntPtr method, void* instance, void** args, out int error);
    public unsafe static void RuntimeInvoke(IntPtr method, void* instancePtr, void** argv, out int error)
    {
        error = 0;

        var m = Ref<MethodInfo>(method);
        if (m == null) {
            error = (int)Error.MissingRequiredArgument;
            return;
        }
        var parameters = m.GetParameters();
        var argc = parameters.Length;

        object? instance = null;
        if (instancePtr != null)
        {
            instance = Ref<object>((IntPtr)instancePtr);
        }

        var args = new object?[argc];
        for (var i = 0; i < argc; i++)
        {
            if (parameters[i].ParameterType.IsValueType) {
                args[i] = ReadValueAsObject(argv[i], parameters[i].ParameterType);
            } else if (parameters[i].ParameterType == typeof(string)) {
                args[i] = Marshal.PtrToStringUTF8((IntPtr)argv[i]);
            } else {
                args[i] = GCHandle.FromIntPtr((IntPtr)argv[i]).Target;
            }
        }

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
