using System;
using Engine;

public class Player {
    public Vector3 Position;

    void Awake() {
        Console.WriteLine($"[C#] Awake: <{Position.x}, {Position.y}, {Position.z}>");
    }

    void Update(float dt) {
        Position.x = (Position.x + dt) % 5;
        Position.y = (Position.y + dt) % 5;
        Position.z = (Position.z + dt) % 5;
        Console.WriteLine($"[C#] Update: <{Position.x}, {Position.y}, {Position.z}>");
    }
}
