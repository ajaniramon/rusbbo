{% if model != null %}{% set title = locale.hk_editar_modelo ~ model.modelId() %}{% else %}{% set title = locale.hk_nuevo_modelo_de_sala %}{% endif %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head"><div></div><div class="actions"><a class="btn" href="{{ hk.path }}/room_models">{{ locale.hk_volver_a_modelos|raw }}</a></div></div>
<div class="card">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/room_models/edit{% if model != null %}?id={{ model.id() }}{% endif %}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row r-3">
        <div class="field"><label>{{ locale.hk_id_del_modelo|raw }}</label><input type="text" name="model_id" class="mono" required value="{% if model != null %}{{ model.modelId() }}{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_nombre|raw }}</label><input type="text" name="model_name" value="{% if model != null %}{{ model.modelName() }}{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_trigger|raw }}</label>
          <select name="trigger_class">{% for t in triggers %}<option value="{{ t }}" {% if model != null and model.triggerClass() == t %}selected{% endif %}>{{ t }}</option>{% endfor %}</select>
        </div>
      </div>
      <div class="row">
        <div class="field"><label>{{ locale.hk_puerta_x|raw }}</label><input type="number" name="door_x" value="{% if model != null %}{{ model.doorX() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_puerta_y|raw }}</label><input type="number" name="door_y" value="{% if model != null %}{{ model.doorY() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_puerta_z|raw }}</label><input type="text" name="door_z" value="{% if model != null %}{{ model.doorZ() }}{% else %}0.0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_direccion_0_7|raw }}</label><input type="number" name="door_dir" min="0" max="7" value="{% if model != null %}{{ model.doorDir() }}{% else %}2{% endif %}"></div>
      </div>
      <div class="field"><label>{{ locale.hk_mapa_de_alturas|raw }}</label><textarea name="heightmap" rows="14" class="mono" style="white-space:pre">{% if model != null %}{{ model.heightmap() }}{% endif %}</textarea><span class="help">{{ locale.hk_una_fila_por_linea_x_casilla_bloqueada_0_9_altur|raw }}</span></div>
      <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_guardar_modelo|raw }}</button></div>
    </form>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
