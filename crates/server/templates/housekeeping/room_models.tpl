{% set title = locale.hk_modelos_de_sala %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <input type="search" data-filter="model-table" placeholder="{{ locale.hk_filtrar_modelos|raw }}">
  <span class="spacer"></span>
  <a class="btn primary" href="{{ hk.path }}/room_models/edit">{{ locale.hk_nuevo_modelo|raw }}</a>
</div>
<div class="card flush">
  <div class="card-body scroll">
    <table class="tbl" id="model-table">
      <thead><tr><th>{{ locale.hk_id|raw }}</th><th>{{ locale.hk_modelo|raw }}</th><th>{{ locale.hk_nombre|raw }}</th><th>{{ locale.hk_puerta_x_y_z_dir|raw }}</th><th>{{ locale.hk_trigger|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for m in models %}
        <tr>
          <td class="muted">{{ m.id() }}</td>
          <td class="mono"><strong>{{ m.modelId() }}</strong></td>
          <td>{{ m.modelName() }}</td>
          <td class="mono small">{{ m.doorX() }}, {{ m.doorY() }}, {{ m.doorZ() }} · {{ m.doorDir() }}</td>
          <td class="mono small">{{ m.triggerClass() }}</td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/room_models/edit?id={{ m.id() }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/room_models/delete?id={{ m.id() }}" data-confirm="{{ locale.hk_borrar_el_modelo|raw }} {{ m.modelId() }}{{ locale.hk_las_salas_que_lo_usen_dejaran_de_cargar|raw }}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm ghost-danger" type="submit">{{ locale.hk_borrar|raw }}</button>
            </form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
